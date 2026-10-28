#!/usr/bin/env bash
# End-to-end prompt flow on the laptop, on two private buses (no real system
# bus, no root, no devices): fp6-permd + fp6-permission-agent (in a user
# unit named fp6-permission-agent.service, as on the phone) + a fake app in
# app-test-org.example.Serial-1.scope + a fake shell answering through the
# agent's action group (what Fp6PermissionPrompt.qml does).
#   nix-shell -p cargo rustc gcc pkg-config glib dbus --run 'test/dbus-test.sh'
# Needs a systemd user manager (systemd-run --user). Exit status: failures.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
T=${CARGO_TARGET_DIR:-$here/rust/target}
(cd "$here/rust" && CARGO_TARGET_DIR=$T cargo build --offline -q) || exit 1
bin=$T/debug
w=$(mktemp -d)
fails=0
pass() { echo "PASS $*"; }
fail() { echo "FAIL $*"; fails=$((fails + 1)); }

cat > "$w/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$w</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
dbus-daemon --config-file="$w/bus.conf" --fork --print-address=3 --print-pid=4 3>"$w/sys.addr" 4>"$w/sys.pid"
dbus-daemon --config-file="$w/bus.conf" --fork --print-address=3 --print-pid=4 3>"$w/ses.addr" 4>"$w/ses.pid"
SYS=$(head -1 "$w/sys.addr"); SES=$(head -1 "$w/ses.addr")
cleanup() {
  systemctl --user stop fp6-permission-agent-test.service 2>/dev/null
  kill "$permd" 2>/dev/null
  kill "$(cat "$w/sys.pid")" "$(cat "$w/ses.pid")" 2>/dev/null
  rm -rf "$w"
}
trap cleanup EXIT

cat > "$w/permd.json" <<EOF
{"uid": $(id -u), "system_apps": ["lomiri-system-settings"],
 "agent_unit": "fp6-permission-agent-test.service",
 "apparmor_parser": "true", "state_dir": "$w/state", "run_dir": "$w/run",
 "dropin_root": "$w/systemd-user", "grace_secs": 2, "prompt_timeout_secs": 8}
EOF
DBUS_SYSTEM_BUS_ADDRESS=$SYS "$bin/fp6-permd" "$w/permd.json" 2>"$w/permd.log" &
permd=$!
systemd-run --user -q --unit=fp6-permission-agent-test --collect \
  -E DBUS_SYSTEM_BUS_ADDRESS="$SYS" -E DBUS_SESSION_BUS_ADDRESS="$SES" -E XDG_RUNTIME_DIR="$w" \
  "$bin/fp6-permission-agent"
sleep 1.5

ses() { DBUS_SESSION_BUS_ADDRESS=$SES "$@"; }
app() {  # run as the fake app (its own app-*.scope)
  systemd-run --user -q --scope --unit="app-test-org.example.Serial-$RANDOM.scope" \
    env DBUS_SYSTEM_BUS_ADDRESS="$SYS" DBUS_SESSION_BUS_ADDRESS="$SES" "$bin/fp6-permctl" "$@"
}
gact() { ses gdbus call --session -d com.fairphone6.Permissions -o /com/fairphone6/Permissions -m org.gtk.Actions."$@"; }
activate() { gact Activate "$1" "[<'$2'>]" '{}' >/dev/null; }
prompt_id() { gact Describe prompt | grep -o '\\"id\\": *[0-9]*\|"id":[0-9]*' | grep -o '[0-9]*$' | head -1; }
answer_next() {  # fake shell: wait for the prompt, ack it, answer
  for _ in $(seq 40); do
    id=$(gact Describe prompt | sed -n 's/.*\\\?"id\\\?":\([0-9]*\).*/\1/p')
    if [ -n "$id" ]; then activate ack "$id"; activate respond "$id $1"; return 0; fi
    sleep 0.2
  done
  return 1
}

grep -q "registered" "$w/permd.log" && pass "agent registered" || fail "agent did not register: $(cat "$w/permd.log")"

# 1. Ask (focus unknown) -> prompt -> "Only while using" -> granted
answer_next while_using & ans=$!
r=$(app request usb); wait $ans
[ "$r" = granted ] && pass "prompt answered while_using -> granted" || fail "request usb: $r"
r=$(app check usb)
[ "$r" = granted ] && pass "remembered (check)" || fail "check usb: $r"

# 2. While using: app goes to the background -> denied after the grace
activate focus org.example.Other
sleep 3
r=$(app check usb)
[ "$r" = denied ] && pass "while_using denied in the background" || fail "background check: $r"
activate focus org.example.Serial
sleep 0.5
r=$(app check usb)
[ "$r" = granted ] && pass "while_using granted in the foreground again" || fail "foreground check: $r"

# 3. Background request for an undecided permission: refused without a prompt
activate focus org.example.Other
sleep 3
r=$(app request contacts)
[ "$r" = denied ] && pass "background request refused quietly" || fail "background request: $r"
gact Describe prompt | grep -q '"id"' && fail "a background prompt appeared" || pass "no background prompt"

# 4. Deny in the prompt is remembered; dismiss is not
activate focus org.example.Serial
sleep 0.5
answer_next deny & ans=$!
r=$(app request files); wait $ans
[ "$r" = denied ] && [ "$(app check files)" = denied ] && pass "deny remembered" || fail "deny: $r"
answer_next dismissed & ans=$!
r=$(app request calendar); wait $ans
[ "$r" = denied ] && [ "$(app check calendar)" = ask ] && pass "dismiss asks again next time" || fail "dismiss: $r $(app check calendar)"

# 5. Prompt timeout (nobody answers) -> denied, still ask
t0=$(date +%s)
r=$(app request sensors)
[ "$r" = denied ] && [ $(( $(date +%s) - t0 )) -ge 7 ] && pass "unanswered prompt times out as denied" || fail "timeout: $r"

# 6. Settings: change, network toggles, reset (through the agent)
activate set "org.example.Serial	usb	deny"
sleep 0.5
[ "$(app check usb)" = denied ] && pass "Settings deny" || fail "Settings deny"
activate net "org.example.Serial	mobile	0"
sleep 0.5
grep -q '"mobile": false' "$w/run/network.json" && pass "network.json written" || fail "network.json: $(cat "$w/run/network.json")"
st=$(ses "$bin/fp6-permctl" state)
echo "$st" | grep -q '"org.example.Serial"' && echo "$st" | grep -q '"kind":"prompted"' && pass "state + history" || fail "state: $st"
activate reset org.example.Serial
sleep 0.5
[ "$(app check usb)" = ask ] && ! grep -q Serial "$w/run/network.json" && pass "reset" || fail "reset"

# 7. Only the agent may administer; apps cannot register as agent
out=$(app set x usb allow 2>&1; DBUS_SYSTEM_BUS_ADDRESS=$SYS gdbus call --system -d org.fairphone.Permissions1 -o /org/fairphone/Permissions1 -m org.fairphone.Permissions1.SetDecision org.example.Serial usb allow 2>&1)
echo "$out" | grep -q AccessDenied && pass "SetDecision from a non-agent refused" || fail "admin check: $out"
out=$(DBUS_SYSTEM_BUS_ADDRESS=$SYS gdbus call --system -d org.fairphone.Permissions1 -o /org/fairphone/Permissions1 -m org.fairphone.Permissions1.RegisterAgent 2>&1)
echo "$out" | grep -q AccessDenied && pass "RegisterAgent from outside the agent unit refused" || fail "register: $out"
out=$(DBUS_SYSTEM_BUS_ADDRESS=$SYS gdbus call --system -d org.fairphone.Permissions1 -o /org/fairphone/Permissions1 -m org.fairphone.Permissions1.OpenDevice /dev/sda rw 2>&1)
echo "$out" | grep -q "not a USB device node" && pass "OpenDevice refuses non-USB nodes" || fail "open: $out"
# 8. System components: Settings never prompts
r=$(systemd-run --user -q --scope --unit="app-test-lomiri\\x2dsystem\\x2dsettings-$RANDOM.scope" env DBUS_SYSTEM_BUS_ADDRESS="$SYS" "$bin/fp6-permctl" request usb)
[ "$r" = granted ] && pass "system app granted without prompt" || fail "system app: $r"
# Login session (SSH / console): the owner, granted
r=$(DBUS_SYSTEM_BUS_ADDRESS=$SYS "$bin/fp6-permctl" check usb)
echo "INFO caller outside app units ($(cut -d: -f3 /proc/self/cgroup)): $r"
[ "$fails" = 0 ] || { echo "--- permd log"; cat "$w/permd.log"; echo "--- agent log"; journalctl --user -u fp6-permission-agent-test -n 30 --no-pager -o cat; }
echo "checks done, $fails failed"
exit "$fails"
