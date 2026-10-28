#!/usr/bin/env bash
# Memory & performance benchmark (phone side, as the phone user, under the
# GUI lock). Launch times of common apps in four states, the RAM picture
# before/after, the Lomiri journal lines. Needs the app-ready Lomiri patch
# (journal "fp6-app-ready: added|first-frame|focus") and measure.sh next to it.
#   bench-phone.sh OUT [APPS...]
# States per app (measure.sh modes):
#   cold      app stopped, page cache dropped (needs sudo: SUDO_ASKPASS helper)
#   warm      app stopped, its files still cached (right after a run)
#   resume    app in the background (suspended if kept ready / predicted)
#   prelaunch app started hidden by fp6-app-ready, then opened
# Changes on the phone: starts/stops the listed apps, drops the page cache,
# raises the display-lock timeout for the round (restored), nothing else.
set -u
here=$(dirname "$(readlink -f "$0")")
out=$1; shift
apps=${*:-lomiri-system-settings lomiri-dialer-app lomiri-messaging-app morph-browser lomiri-camera-app lomiri-gallery-app lomiri-filemanager-app}
modes=${MODES:-cold warm resume prelaunch}
mkdir -p "$out"
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
exec 9>"$XDG_RUNTIME_DIR/fp6-gui.lock"
flock -w 600 9 || { echo "no GUI lock"; exit 4; }
sysgs() {
  local dir; dir=$(ls -d /nix/store/*-lomiri-schemas-*/share/gsettings-schemas/*/glib-2.0/schemas 2>/dev/null | head -1)
  GSETTINGS_SCHEMA_DIR=$dir GIO_EXTRA_MODULES=$(ls -d /nix/store/*-dconf-*-lib/lib/gio/modules | head -1) \
    "$(ls /nix/store/*-glib-*-bin/bin/gsettings | head -1)" "$@"
}
old_timeout=$(sysgs get com.lomiri.touch.system activity-timeout 2>/dev/null | awk '{print $NF}')
trap '[ -n "$old_timeout" ] && sysgs set com.lomiri.touch.system activity-timeout "$old_timeout"' EXIT
sysgs set com.lomiri.touch.system activity-timeout 600 2>/dev/null
sh "$here/snapshot.sh" > "$out/ram-before.txt" 2>&1
t0=$(date +%s)
for app in $apps; do
  echo "[$(date +%T)] $app: $modes"
  bash "$here/measure.sh" "$out/$app" "$app" $modes 2>>"$out/errors.log" | tee -a "$out/results.txt"
  # measure.sh leaves the app running; stop it (every instance of a
  # multi-instance app) so the next app starts from the same picture
  for u in /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice/lomiri-app-launch--application-*--"$app"--*.service; do
    [ -d "$u" ] && timeout 8 systemctl --user stop "$(basename "$u")" 2>/dev/null
  done
done
sh "$here/snapshot.sh" > "$out/ram-after.txt" 2>&1
journalctl --user -u lomiri.service --since "@$t0" --no-pager -o short-monotonic 2>/dev/null | grep "fp6-app-ready:" > "$out/lomiri-journal.txt"
journalctl --user -u fp6-app-ready --since "@$t0" --no-pager -o short-monotonic > "$out/daemon-journal.txt" 2>/dev/null
# summary: app, then first_frame (cold, warm) and focus (resume, prelaunch) in ms
awk '{ split($3, f, "="); split($4, g, "="); v = ($2 == "resume" || $2 == "prelaunch") ? g[2] : f[2]; t[$1, $2] = v; seen[$1] = 1 }
     END { printf "%-26s %8s %8s %8s %9s\n", "app", "cold", "warm", "resume", "prelaunch";
           for (a in seen) printf "%-26s %8s %8s %8s %9s\n", a, t[a, "cold"], t[a, "warm"], t[a, "resume"], t[a, "prelaunch"] }' \
  "$out/results.txt" | tee "$out/summary.txt"
