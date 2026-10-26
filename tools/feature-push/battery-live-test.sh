#!/usr/bin/env bash
# Live test of fp6-batteryd on the FP6 WITHOUT activating anything: the
# daemon runs as a transient root unit on a private D-Bus bus (no system
# bus policy needed), with its own state/run directories under
# /run/fp6-battery-test; the agent runs as a transient user unit with that
# bus as its "system bus". Real effects: charge thresholds and cpufreq caps
# (both restored by "stop"), and the user's dconf keys under
# /com/fairphone6/battery/ (reset by "stop").
#
#   tools/feature-push/battery-live-test.sh start /nix/store/...-fp6-batteryd-0.1.0
#   tools/feature-push/battery-live-test.sh set KEY VALUE    # gsettings set com.fairphone6.battery
#   tools/feature-push/battery-live-test.sh alarm UNIX       # fake next alarm (stop the agent first)
#   tools/feature-push/battery-live-test.sh status
#   tools/feature-push/battery-live-test.sh stop             # restore everything
set -euo pipefail
here=$(cd "$(dirname "$0")/../.." && pwd)
ssh() { "$here/tools/phone-ssh" "$@"; }
root() {
  local enc pw
  pw=$(head -n1 "$here/.fp6-phone-secret")
  enc=$(printf '%s' "$1" | base64 -w0)
  printf '%s\n' "$pw" | ssh "sudo -S -p '' sh -c \"\$(echo $enc | base64 -d)\""
}
T=/run/fp6-battery-test
BUS="unix:path=$T/bus"
USERENV="export XDG_RUNTIME_DIR=/run/user/10000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10000/bus"
cmd=${1:?start|set|alarm|status|stop}
case $cmd in
start)
  pkg=${2:?package path}
  root "set -e; mkdir -p $T/state $T/run; chmod 755 $T $T/run
cat > $T/bus.conf <<'XML'
<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">
<busconfig>
  <type>fp6-battery-test</type>
  <listen>unix:path=$T/bus</listen>
  <auth>EXTERNAL</auth>
  <policy context=\"default\">
    <allow user=\"root\"/><allow user=\"user\"/>
    <allow own=\"*\"/><allow send_destination=\"*\" eavesdrop=\"false\"/><allow receive_sender=\"*\"/>
  </policy>
</busconfig>
XML
export PATH=/run/current-system/sw/bin
systemctl stop fp6-batteryd-test fp6-batteryd-test-bus 2>/dev/null || true
systemd-run --unit=fp6-batteryd-test-bus --collect -E PATH=\$PATH dbus-daemon --nofork --config-file=$T/bus.conf
for i in \$(seq 50); do [ -S $T/bus ] && break; sleep 0.1; done
chmod 666 $T/bus
systemd-run --unit=fp6-batteryd-test --collect -E PATH=\$PATH -E DBUS_SESSION_BUS_ADDRESS=$BUS \
  $pkg/bin/fp6-batteryd daemon --session-bus --state-dir $T/state --run-dir $T/run --gps-unit fp6-gnss-nmea.service"
  ssh "$USERENV; systemctl --user stop fp6-batteryd-test-agent 2>/dev/null || true
systemd-run --user --unit=fp6-batteryd-test-agent --collect -E DBUS_SYSTEM_BUS_ADDRESS=$BUS \
  $pkg/bin/fp6-batteryd agent --status $T/run/status.json; echo $pkg > \$XDG_RUNTIME_DIR/fp6-battery-test.pkg"
  ;;
set)
  ssh "$USERENV; pkg=\$(cat \$XDG_RUNTIME_DIR/fp6-battery-test.pkg); GSETTINGS_SCHEMA_DIR=\$pkg/share/fp6-batteryd/agent-schemas \
    gsettings set com.fairphone6.battery ${2:?key} ${3:?value}"
  ;;
alarm)
  root "DBUS_SESSION_BUS_ADDRESS=$BUS /run/current-system/sw/bin/busctl --user call com.fairphone6.Battery /com/fairphone6/Battery com.fairphone6.Battery SetNextAlarm x ${2:?unix}"
  ;;
status)
  ssh "cat $T/run/status.json; echo; for p in /sys/devices/system/cpu/cpufreq/policy*; do echo \$p \$(cat \$p/scaling_max_freq)/\$(cat \$p/cpuinfo_max_freq); done
B=/sys/class/power_supply/qcom-battmgr-bat; echo thresholds \$(cat \$B/charge_control_start_threshold)/\$(cat \$B/charge_control_end_threshold)
cat $T/run/saver 2>/dev/null"
  ;;
stop)
  ssh "$USERENV; systemctl --user stop fp6-batteryd-test-agent 2>/dev/null || true; dconf reset -f /com/fairphone6/battery/ || true; rm -f \$XDG_RUNTIME_DIR/fp6-battery-test.pkg"
  root "export PATH=/run/current-system/sw/bin; systemctl stop fp6-batteryd-test fp6-batteryd-test-bus 2>/dev/null || true
B=/sys/class/power_supply/qcom-battmgr-bat; echo 100 > \$B/charge_control_end_threshold; echo 95 > \$B/charge_control_start_threshold
for p in /sys/devices/system/cpu/cpufreq/policy*; do cat \$p/cpuinfo_max_freq > \$p/scaling_max_freq; done
echo thresholds \$(cat \$B/charge_control_start_threshold)/\$(cat \$B/charge_control_end_threshold)
for p in /sys/devices/system/cpu/cpufreq/policy*; do echo \$p \$(cat \$p/scaling_max_freq)/\$(cat \$p/cpuinfo_max_freq); done
rm -rf $T"
  ;;
*) echo "unknown command $cmd" >&2; exit 2 ;;
esac
