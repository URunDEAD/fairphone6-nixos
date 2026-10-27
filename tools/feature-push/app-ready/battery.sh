#!/usr/bin/env bash
# Idle battery draw (phone side, inside a round.sh hold via SETUP):
#   APPS='a b c' SECONDS_IDLE=360 OUT=file battery.sh
# Opens APPS one after another (12 s each), puts them in the background by
# opening lomiri-clock-app, turns the display off, samples the battery
# (current_now, voltage_now, charge_counter) every 5 s, turns the display
# back on and prints the averages, plus the CPU time the app cgroups and the
# whole system used meanwhile (cpu.stat usage_usec, /proc/stat): on USB power
# the battery current is ~0 (the charger feeds the system at 100 %), so CPU
# time is the measurable proxy for the battery cost. The caller stops the apps.
set -u
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
export XDG_RUNTIME_DIR=/run/user/$(id -u)
LAL=$(dirname "$(ls /nix/store/*-lomiri-app-launch-*/bin/lomiri-app-launch | head -1)")
B=/sys/class/power_supply/qcom-battmgr-bat
out=${OUT:-/dev/stdout}
screen() {
  asroot gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
    -m com.canonical.Unity.Screen.setScreenPowerMode "$1" 0 >/dev/null 2>&1
}
for a in ${APPS:-}; do timeout 25 "$LAL/lomiri-app-launch" "$a" >/dev/null 2>&1; sleep 12; done
timeout 25 "$LAL/lomiri-app-launch" lomiri-clock-app >/dev/null 2>&1; sleep 10
stopped=0
for d in /sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice/lomiri-app-launch--*; do
  for p in $(cat "$d/cgroup.procs" 2>/dev/null); do grep -q '^State:.*T' /proc/$p/status && stopped=$((stopped + 1)); done
done
screen off; sleep 20
c0=$(cat $B/charge_counter); t0=$(date +%s); n=0; isum=0; psum=0
end=$(( t0 + ${SECONDS_IDLE:-360} ))
cpu0=$(awk '/^cpu /{print $2+$3+$4+$6+$7+$8}' /proc/stat)
slice=/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice
appcpu() { local t=0 u; for d in "$slice"/lomiri-app-launch--*; do u=$(awk '/^usage_usec/{print $2}' "$d/cpu.stat" 2>/dev/null); t=$(( t + ${u:-0} )); done; echo $t; }
shellcpu() { awk '/^usage_usec/{print $2}' "$slice/lomiri.service/cpu.stat"; }
a0=$(appcpu); s0=$(shellcpu); w0=$(awk '/^ctxt/{print $2}' /proc/stat)
while [ "$(date +%s)" -lt $end ]; do
  i=$(cat $B/current_now); v=$(cat $B/voltage_now)
  isum=$(( isum + i )); psum=$(( psum + i / 1000 * (v / 1000) / 1000 )); n=$((n + 1))
  sleep 5
done
cpu1=$(awk '/^cpu /{print $2+$3+$4+$6+$7+$8}' /proc/stat)
a1=$(appcpu); s1=$(shellcpu); w1=$(awk '/^ctxt/{print $2}' /proc/stat)
c1=$(cat $B/charge_counter); t1=$(date +%s)
screen on
echo "apps='${APPS:-}' stopped_procs=$stopped samples=$n avg_current_uA=$(( isum / n )) avg_power_mW=$(( psum / n )) charge_counter_delta_uAh=$(( c1 - c0 )) over_s=$(( t1 - t0 )) busy_jiffies=$(( cpu1 - cpu0 )) apps_cpu_ms=$(( (a1 - a0) / 1000 )) shell_cpu_ms=$(( (s1 - s0) / 1000 )) ctxt_switches=$(( w1 - w0 )) usb_online=$(cat /sys/class/power_supply/qcom-battmgr-usb/online) status=$(cat $B/status)" | tee -a "$out"
