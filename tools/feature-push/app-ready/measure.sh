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
set -u
out=$1 app=$2; shift 2
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
LAL=${LAL:-$(dirname "$(ls /nix/store/*-lomiri-app-launch-*/bin/lomiri-app-launch | head -1)")}
OTHER=${OTHER:-lomiri-clock-app}
APPREADY=${APPREADY:-fp6-app-ready}
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
slice=/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice
unit_dir() { ls -d "$slice"/lomiri-app-launch--application-*--"$1"--*.service 2>/dev/null | head -1; }
running() { d=$(unit_dir "$1"); [ -n "$d" ] && [ -s "$d/cgroup.procs" ]; }
unit_name() { d=$(unit_dir "$1"); [ -n "$d" ] && basename "$d"; }
# Stop the app's unit synchronously (lomiri-app-stop returns before the unit
# is gone, and a launch while it is still deactivating queues behind the stop).
stop_app() {
  local u; u=$(unit_name "$1") || return 0
  [ -n "$u" ] || return 0
  timeout 8 systemctl --user stop "$u" 2>/dev/null || {
    systemctl --user kill -s KILL "$u" 2>/dev/null; sleep 1
    timeout 5 systemctl --user stop "$u" 2>/dev/null; }
  systemctl --user reset-failed "$u" 2>/dev/null
  for i in $(seq 1 20); do running "$1" || break; sleep 0.25; done
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
      timeout 25 "$LAL/lomiri-app-launch" "$OTHER" >/dev/null 2>&1; sleep 4
      "$APPREADY" prelaunch "$app" >> "$out/prelaunch.log" 2>&1
      sleep 12
      echo "$app prelaunch: focused after hidden launch: $(cat "$XDG_RUNTIME_DIR/fp6-app-ready/focused" 2>/dev/null)" | tee -a "$out/results.txt" ;;
    resume)
      running "$app" || { timeout 25 "$LAL/lomiri-app-launch" "$app" >/dev/null 2>&1; sleep 12; }
      timeout 25 "$LAL/lomiri-app-launch" "$OTHER" >/dev/null 2>&1; sleep 6 ;;
  esac
  d=$(unit_dir "$app")
  frozen=-
  if { [ "$mode" = resume ] || [ "$mode" = prelaunch ]; } && [ -n "$d" ]; then
    frozen=yes
    for p in $(cat "$d/cgroup.procs"); do grep -q '^State:.*T' /proc/$p/status || frozen=no; done
  fi
  t0=$(date +%s%6N)
  timeout 25 "$LAL/lomiri-app-launch" "$app" >/dev/null 2>&1 &
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
