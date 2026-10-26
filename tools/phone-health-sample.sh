#!/bin/sh
# Read-only, identifier-free sysfs sample. Run locally on FP6 or through SSH.
# Values retain kernel units: voltage/current micro-units, temperatures as
# provided by each ABI (power_supply temp=tenths C, thermal temp=millidegrees C).
set -u
emit() { printf '%s\t%s\n' "$1" "$2"; }
read_attr() {
    health_key=$1
    health_path=$2
    test -r "$health_path" || return 0
    if health_value=$(cat "$health_path" 2>/dev/null); then
        emit "$health_key" "$health_value"
    else
        emit "$health_key.error" unavailable
    fi
}
emit phone_time "$(date --iso-8601=seconds)"
emit kernel.release "$(uname -r)"
emit system.current "$(readlink -f /run/current-system)"
emit system.profile "$(readlink -f /nix/var/nix/profiles/system)"
read_attr boot_id /proc/sys/kernel/random/boot_id
read_attr uptime /proc/uptime
read_attr loadavg /proc/loadavg
read_attr wifi.operstate /sys/class/net/wlan0/operstate
read_attr wifi.carrier /sys/class/net/wlan0/carrier
for health_counter in rx_bytes tx_bytes rx_errors tx_errors rx_dropped tx_dropped; do
    read_attr "wifi.$health_counter" "/sys/class/net/wlan0/statistics/$health_counter"
done
emit memory.summary "$(sed -n '/^MemTotal:/p;/^MemAvailable:/p;/^SwapTotal:/p;/^SwapFree:/p' /proc/meminfo)"
emit system.state "$(systemctl is-system-running 2>/dev/null || true)"
emit system.failed_units "$(systemctl --failed --no-pager --plain --no-legend 2>/dev/null || true)"
for health_unit in display-manager phosh NetworkManager bluetooth rmtfs ModemManager systemd-timesyncd; do
    emit "service.$health_unit" "$(systemctl show "$health_unit.service" --no-pager -p ActiveState -p SubState -p NRestarts -p Result 2>/dev/null || true)"
done
if test -S /run/user/10000/bus; then
    emit user.failed_units "$(env XDG_RUNTIME_DIR=/run/user/10000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10000/bus systemctl --user --failed --no-pager --plain --no-legend 2>/dev/null || true)"
    for health_unit in lomiri lomiri-keyboard mobi.phosh.Shell mobi.phosh.OSK pipewire pipewire-pulse wireplumber; do
        emit "user.service.$health_unit" "$(env XDG_RUNTIME_DIR=/run/user/10000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10000/bus systemctl --user show "$health_unit.service" --no-pager -p ActiveState -p SubState -p MainPID -p NRestarts -p Result 2>/dev/null || true)"
    done
fi
read_attr gpu.runtime_status /sys/bus/platform/devices/3d00000.gpu/power/runtime_status
read_attr display.runtime_status /sys/class/drm/card0/device/power/runtime_status
read_attr display.brightness /sys/class/backlight/ae94000.dsi.0/brightness
read_attr torch.brightness /sys/class/leds/white:flash/brightness
for health_supply in /sys/class/power_supply/*; do
    test -d "$health_supply" || continue
    for health_attr in type status online capacity health voltage_now current_now power_now energy_now charge_now charge_full charge_full_design temp usb_type cycle_count; do
        read_attr "power_supply.${health_supply##*/}.$health_attr" "$health_supply/$health_attr"
    done
done
for health_thermal in /sys/class/thermal/thermal_zone*; do
    test -d "$health_thermal" || continue
    health_type=$(cat "$health_thermal/type" 2>/dev/null) || continue
    case "$health_type" in
        cpuss0-thermal|gpuss0-thermal|modem0-thermal|qcom-battmgr-bat)
            read_attr "thermal.$health_type.temp" "$health_thermal/temp" ;;
    esac
done
for health_proc in /sys/class/remoteproc/remoteproc*; do
    test -d "$health_proc" || continue
    health_name=$(cat "$health_proc/name" 2>/dev/null) || continue
    read_attr "remoteproc.$health_name.state" "$health_proc/state"
done
read_attr rtc.since_epoch /sys/class/rtc/rtc0/since_epoch
read_attr memory.pressure /proc/pressure/memory
read_attr io.pressure /proc/pressure/io
