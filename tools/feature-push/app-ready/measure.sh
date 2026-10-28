#!/usr/bin/env bash
# App launch measurement (phone side, as the phone user, under the GUI lock
# held by the caller). Needs the app-ready Lomiri (journal lines
# "fp6-app-ready: added|first-frame|focus <appid>").
#   measure.sh OUT APP MODE...
# MODE: cold     app stopped, page cache dropped
#       warm     app stopped, page cache warm (right after a previous run)
#       preload  app stopped, cache dropped, then `fp6-app-ready preload APP`
#       resume   app running in the background (another app focused first);
#                time to Lomiri focus + whether the app was frozen
#       prelaunch app stopped, another app focused, `fp6-app-ready prelaunch APP`
#                (hidden launch); checks focus stayed on the other app, then
#                measures the first open like resume
# Prints one line per run: APP MODE first_frame_ms focus_ms frozen mem_MiB
# Opening an app that already runs (resume, prelaunch, OTHER): a
# multi-instance app (no X-Lomiri-Single-Instance, e.g. Clock, Calendar,
# Calculator; unit ...--<appid>--<instance>.service) would get a new instance
# from `lomiri-app-launch APP`, so its running instance is resumed and
# focused the way lomiri-app-launch does it for a single-instance app:
# LomiriResumeRequest + LomiriFocusRequest (appid, instance) on the session
# bus. Single-instance apps still go through `lomiri-app-launch APP`.
set -u
out=$1 app=$2; shift 2
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
LAL=${LAL:-$(dirname "$(ls /nix/store/*-lomiri-app-launch-*/bin/lomiri-app-launch | head -1)")}
OTHER=${OTHER:-lomiri-clock-app}
APPREADY=${APPREADY:-fp6-app-ready}
GDBUS=${GDBUS:-$(command -v gdbus || ls /nix/store/*-glib-*-bin/bin/gdbus 2>/dev/null | head -1)}
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
slice=/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice
unit_dir() { ls -d "$slice"/lomiri-app-launch--application-*--"$1"--*.service 2>/dev/null | head -1; }
running() { d=$(unit_dir "$1"); [ -n "$d" ] && [ -s "$d/cgroup.procs" ]; }
unit_name() { d=$(unit_dir "$1"); [ -n "$d" ] && basename "$d"; }
# Instance id of the app's running unit ("" for a single-instance app).
instance_of() {
  local u; u=$(unit_name "$1") || return 1
  u=${u%.service}; echo "${u##*--}"
}
# Stop every unit of the app synchronously (lomiri-app-stop returns before the
# unit is gone, and a launch while it is still deactivating queues behind the
# stop; multi-instance apps can have several).
stop_app() {
  local d u
  for d in "$slice"/lomiri-app-launch--application-*--"$1"--*.service; do
    [ -d "$d" ] || continue
    u=$(basename "$d")
    timeout 8 systemctl --user stop "$u" 2>/dev/null || {
      systemctl --user kill -s KILL "$u" 2>/dev/null; sleep 1
      timeout 5 systemctl --user stop "$u" 2>/dev/null; }
    systemctl --user reset-failed "$u" 2>/dev/null
  done
  for i in $(seq 1 20); do running "$1" || break; sleep 0.25; done
}
# Open APP: resume + focus its running instance if it is a multi-instance app
# that runs, else `lomiri-app-launch APP` (start, or second start of a
# single-instance app). Returns at once for the signal path; lomiri-app-launch
# runs in the background when $2 = bg.
open_app() {
  local inst
  if running "$1" && inst=$(instance_of "$1") && [ -n "$inst" ] && [ -n "$GDBUS" ]; then
    "$GDBUS" emit --session --object-path / --signal com.lomiri.LomiriAppLaunch.LomiriResumeRequest "$1" "$inst" >/dev/null 2>&1
    "$GDBUS" emit --session --object-path / --signal com.lomiri.LomiriAppLaunch.LomiriFocusRequest "$1" "$inst" >/dev/null 2>&1
  elif [ "${2:-}" = bg ]; then
    timeout 25 "$LAL/lomiri-app-launch" "$1" >/dev/null 2>&1 &
  else
    timeout 25 "$LAL/lomiri-app-launch" "$1" >/dev/null 2>&1
  fi
}
# journal realtime (us) of the first matching Lomiri line since $1 (us)
jtime() {
  journalctl --user -u lomiri.service --since "@$(( $1 / 1000000 ))" -o json --no-pager 2>/dev/null \
    | grep -F "fp6-app-ready: $2 $app\"" | head -1 | sed -n 's/.*"__REALTIME_TIMESTAMP":"\([0-9]*\)".*/\1/p'
}
wait_line() {  # wait_line T0 KIND TIMEOUT_S
  local t=0 v
  while [ $t -lt $(( $3 * 4 )) ]; do
    v=$(jtime "$1" "$2"); [ -n "$v" ] && { echo "$v"; return 0; }
    sleep 0.25; t=$((t + 1))
  done
  return 1
}
mkdir -p "$out"
for mode in "$@"; do
  case $mode in
    cold|preload)
      stop_app "$app"; sync; asroot sh -c 'echo 3 > /proc/sys/vm/drop_caches'; sleep 2
      if [ "$mode" = preload ]; then "$APPREADY" preload "$app" >> "$out/preload.log" 2>&1; sleep 6; fi ;;
    warm) stop_app "$app"; sleep 1 ;;
    prelaunch)
      stop_app "$app"
      open_app "$OTHER"; sleep 4
      "$APPREADY" prelaunch "$app" >> "$out/prelaunch.log" 2>&1
      sleep 12
      echo "$app prelaunch: focused after hidden launch: $(cat "$XDG_RUNTIME_DIR/fp6-app-ready/focused" 2>/dev/null)" | tee -a "$out/results.txt" ;;
    resume)
      running "$app" || { open_app "$app"; sleep 12; }
      open_app "$OTHER"; sleep "${RESUME_WAIT:-6}" ;;
  esac
  d=$(unit_dir "$app")
  frozen=-
  if { [ "$mode" = resume ] || [ "$mode" = prelaunch ]; } && [ -n "$d" ]; then
    frozen=yes
    for p in $(cat "$d/cgroup.procs"); do grep -q '^State:.*T' /proc/$p/status || frozen=no; done
  fi
  t0=$(date +%s%6N)
  open_app "$app" bg
  if [ "$mode" = resume ] || [ "$mode" = prelaunch ]; then
    tf=$(wait_line "$t0" focus 10); ff=-
    sleep 3
  else
    ff=$(wait_line "$t0" first-frame 40); tf=$(jtime "$t0" focus)
    sleep 8
  fi
  d=$(unit_dir "$app")
  mem=$(( $(cat "$d/memory.current" 2>/dev/null || echo 0) / 1048576 ))
  ffms=-; [ -n "${ff:-}" ] && [ "$ff" != - ] && ffms=$(( (ff - t0) / 1000 ))
  tfms=-; [ -n "${tf:-}" ] && tfms=$(( (tf - t0) / 1000 ))
  echo "$app $mode first_frame_ms=$ffms focus_ms=$tfms frozen=$frozen mem_MiB=$mem" | tee -a "$out/results.txt"
  wait
done
