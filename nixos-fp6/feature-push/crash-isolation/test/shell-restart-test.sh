#!/usr/bin/env bash
# Laptop test of the shell-restart session loop (lomiri-session-shell-restart.patch)
# and of fp6-crash-notify, against MOCK user units in the laptop's own user
# manager ($XDG_RUNTIME_DIR/systemd/user/fp6ci-*; removed at the end). Nothing
# here touches the real graphical session, Lomiri or the phone.
#
#   test/shell-restart-test.sh <patched run-systemd-session> [fp6-crash-notify]
#
# Mock shell: Type=notify, BindsTo/Before a StopWhenUnneeded target with a
# PartOf "session service", Restart=on-failure and the start limit
# crash-isolation-fp6.nix gives lomiri.service. Crashes are SIGSEGV/SIGABRT the
# mock sends itself (the coredump handler may record them; they are tiny shell
# processes).
set -euo pipefail
script=$1
notify=${2:-}
units=$XDG_RUNTIME_DIR/systemd/user
work=$(mktemp -d)
trap 'systemctl --user stop fp6ci-shell.service fp6ci-svc.service fp6ci-app--application-legacy--mockapp--.service 2>/dev/null || true
      rm -rf "$units"/fp6ci-* "$work"; systemctl --user daemon-reload; [ -z "${npid:-}" ] || kill "$npid" 2>/dev/null || true' EXIT
mkdir -p "$units"
fail() { echo "FAIL: $*"; exit 1; }

# The loop exactly as patched: from the begin marker to the end of the while.
sed -n '/^# fp6-shell-wait-begin/,/^trap - TERM HUP INT$/p' "$script" | grep -v '^trap - TERM HUP INT$' > "$work/loop.sh"
grep -q 'shell_restarting' "$work/loop.sh" || fail "loop not found in $script"

cat > "$work/shell.sh" <<EOF
#!/bin/sh
# \$1 = how many runs crash before one exits cleanly (0 = always crash)
n=\$(cat "$work/count" 2>/dev/null || echo 0); n=\$((n + 1)); echo \$n > "$work/count"
systemd-notify --ready
sleep 1
if [ "\$1" = 0 ] || [ \$n -le "\$1" ]; then kill -SEGV \$\$; fi
sleep 2
EOF
chmod +x "$work/shell.sh"
cat > "$units/fp6ci-gs.target" <<EOF
[Unit]
StopWhenUnneeded=yes
RefuseManualStart=yes
EOF
cat > "$units/fp6ci-svc.service" <<EOF
[Unit]
PartOf=fp6ci-gs.target
After=fp6ci-gs.target
[Service]
ExecStart=/bin/sh -c 'date +%s.%N >> $work/svc-starts; exec sleep 600'
EOF
mkdir -p "$units/fp6ci-gs.target.wants"
ln -sf "$units/fp6ci-svc.service" "$units/fp6ci-gs.target.wants/"

mock_shell() { # $1 = crashing runs
  cat > "$units/fp6ci-shell.service" <<EOF
[Unit]
BindsTo=fp6ci-gs.target
Before=fp6ci-gs.target
StartLimitIntervalSec=300
StartLimitBurst=4
[Service]
Type=notify
NotifyAccess=all
ExecStart=$work/shell.sh $1
Restart=on-failure
RestartSec=1
EOF
  systemctl --user daemon-reload
  systemctl --user reset-failed fp6ci-shell.service 2>/dev/null || true
  rm -f "$work/count" "$work/svc-starts"
}

# session.sh runs the patched block itself (function, start --wait, while).
printf 'set -e\n. "%s"\n' "$work/loop.sh" > "$work/session.sh"
grep -q '^while shell_restarting "\$1"; do$' "$work/loop.sh" || fail "patched loop not found"

# 1. Two crashes, then a clean exit: one session, three shell runs, the
#    session service started once (graphical target never dropped).
mock_shell 2
t0=$(date +%s)
bash "$work/session.sh" fp6ci-shell.service
[ "$(cat "$work/count")" = 3 ] || fail "case 1: shell ran $(cat "$work/count") times, want 3"
[ "$(wc -l < "$work/svc-starts")" = 1 ] || fail "case 1: session service started $(wc -l < "$work/svc-starts") times"
echo "ok 1: survived 2 shell crashes in one session ($(( $(date +%s) - t0 )) s), session service kept"

# 2. Crashing forever: the start limit ends the session (logout, as before).
mock_shell 0
bash "$work/session.sh" fp6ci-shell.service
[ "$(cat "$work/count")" = 4 ] || fail "case 2: shell ran $(cat "$work/count") times, want 4 (start limit)"
[ "$(systemctl --user show -p Result --value fp6ci-shell.service)" = start-limit-hit ] || fail "case 2: no start-limit-hit"
echo "ok 2: crash loop stops at the start limit and ends the session"

# 2b. The next login resets the limit (run-systemd-session's reset-failed).
grep -q 'reset-failed "\$1"' "$script" || fail "case 2b: no reset-failed of the shell unit"
systemctl --user reset-failed fp6ci-shell.service
mock_shell 0; mock_shell 1   # one crash then clean
bash "$work/session.sh" fp6ci-shell.service
[ "$(cat "$work/count")" = 2 ] || fail "case 2b: shell ran $(cat "$work/count") times after reset, want 2"
echo "ok 2b: after reset-failed a new session starts and survives a crash"

# 3. A deliberate stop (logout) ends the loop.
cat > "$work/shell.sh" <<'EOF'
#!/bin/sh
systemd-notify --ready
exec sleep 600
EOF
mock_shell 1
( sleep 2; systemctl --user stop fp6ci-shell.service ) &
timeout 20 bash "$work/session.sh" fp6ci-shell.service || fail "case 3: loop did not end on stop"
echo "ok 3: a stop ends the session"

# 4. fp6-crash-notify (dry run): an app unit dying of SIGSEGV is reported once,
#    SIGKILL (low-memory killer / OOM) and exit codes are not; a shell crash
#    is reported; the Report action starts the report unit.
if [ -n "$notify" ]; then
  mkfifo "$work/in"
  "$notify" --dry-run --app-prefix fp6ci-app-- --shell-unit fp6ci-shell.service --report-unit fp6ci-report.service \
    < "$work/in" > "$work/notify.out" 2> "$work/notify.err" &
  npid=$!
  exec 9> "$work/in"
  for _ in $(seq 50); do grep -q READY "$work/notify.out" 2>/dev/null && break; sleep 0.1; done
  grep -q READY "$work/notify.out" || fail "case 4: notifier not ready: $(cat "$work/notify.err")"
  app=fp6ci-app--application-legacy--mockapp--.service
  # (A script: systemd-run would turn "$$" into "$".)
  printf '#!/bin/sh\nsleep 0.3\n[ "$1" = exit ] && exit 3\nkill -"$1" $$\n' > "$work/app.sh"
  chmod +x "$work/app.sh"
  for how in SEGV KILL TERM exit; do
    systemd-run --user --quiet --unit="$app" --collect -p Type=exec "$work/app.sh" "$how" || true
    sleep 1.5
    systemctl --user reset-failed "$app" 2>/dev/null || true
  done
  cat > "$work/shell.sh" <<'EOF'
#!/bin/sh
systemd-notify --ready
sleep 0.5
kill -ABRT $$
EOF
  mock_shell 0
  sed -i 's/^Restart=on-failure/Restart=no/' "$units/fp6ci-shell.service"; systemctl --user daemon-reload
  systemctl --user start fp6ci-shell.service || true
  sleep 4.5
  apps=$(grep -c 'key=mockapp ' "$work/notify.out" || true)
  [ "$apps" = 1 ] || fail "case 4: $apps app notifications, want 1 (SEGV only): $(cat "$work/notify.out")"
  grep -q 'summary="mockapp stopped"' "$work/notify.out" || fail "case 4: summary"
  grep -q 'key=shell ' "$work/notify.out" || fail "case 4: no shell notification: $(cat "$work/notify.err")"
  id=$(grep -m1 'key=mockapp ' "$work/notify.out" >/dev/null && echo 1)
  echo "invoke $id" >&9
  sleep 0.5
  grep -q 'REPORT unit=fp6ci-report.service' "$work/notify.out" || fail "case 4: Report action did nothing"
  echo "ok 4: notifier: SIGSEGV reported once, SIGKILL/SIGTERM/exit code ignored, shell crash reported, Report starts the report unit"
fi
echo PASS
