#!/bin/sh
# READ-ONLY. Snapshot of every misc feature's phone-side state (~30 s).
# No writes, no service changes, no mounts, no suspend. Safe any time.
. "$(dirname "$0")/misc-common.sh"
out="$OUT_DIR/survey-$(stamp).txt"
PH_TIMEOUT=120 ph "
echo '## kernel'; uname -r; cat /proc/sys/kernel/random/boot_id
echo '## rtc'; for f in name since_epoch date time hctosys; do echo \"\$f=\$(cat /sys/class/rtc/rtc0/\$f)\"; done; echo wall=\$(date -u +%s) synced=\$(timedatectl show -p NTPSynchronized --value)
echo '## battery'; cd /sys/class/power_supply/qcom-battmgr-bat; for f in status capacity current_now voltage_now charge_counter charge_full charge_full_design charge_now; do echo \"\$f=\$(cat \$f 2>&1)\"; done; cd /
echo usb_online=\$(cat /sys/class/power_supply/qcom-battmgr-usb/online) usb_type=\$(cat /sys/class/power_supply/qcom-battmgr-usb/usb_type)
upower -i /org/freedesktop/UPower/devices/DisplayDevice | grep -E 'state|percentage|energy|time to'
echo '## sd'; ls /sys/bus/mmc/devices/ 2>&1; lsblk -rno NAME,SIZE,FSTYPE,LABEL,MOUNTPOINTS /dev/mmcblk0 2>&1; $SUDO cat /sys/kernel/debug/mmc0/ios 2>&1 | head -5; findmnt -rn -o TARGET,SOURCE | grep -E '^/(run/)?media' || echo no-media-mounts
echo '## suspend'; cat /sys/power/mem_sleep; grep -H . /sys/power/suspend_stats/success /sys/power/suspend_stats/fail /sys/power/suspend_stats/last_failed_errno | sed 's|.*/||'
for m in CanSuspend CanPowerOff CanReboot; do echo \"\$m \$(busctl call org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager \$m)\"; done
echo ufs_spm_lvl=\$(cat /sys/bus/platform/devices/1d84000.ufshc/spm_lvl) ufs_hibern8_exit_failures_this_boot=\$($SUDO journalctl -b -k --no-pager | grep -c 'hibern8 exit failed')
$SUDO journalctl -b --no-pager | grep -E 'suspend requested from' | tail -3
echo '## typec/display'; cat /sys/class/typec/port0/data_role /sys/class/typec/port0/power_role; ls /sys/class/typec_mux; ls -d /sys/class/typec/port0-partner/port0-partner.* 2>/dev/null || echo no-altmodes; ls /sys/class/drm | grep -E 'DP|HDMI' || echo no-external-connectors
echo '## video'; for v in /sys/class/video4linux/video*; do n=\$(cat \$v/name); case \$n in qcom-iris*) echo \"\$v \$n\";; esac; done
echo '## apparmor'; cat /sys/module/apparmor/parameters/enabled; $SUDO cat /sys/kernel/security/apparmor/profiles | awk '{print \$NF}' | sort | uniq -c
echo '## adb/mtp'; systemctl is-active adbd fp6-mtp 2>&1 | tr '\n' ' '; echo; ls /sys/kernel/config/usb_gadget/g1/configs/c.1/ | tr '\n' ' '; echo UDC=\$(cat /sys/kernel/config/usb_gadget/g1/UDC)
echo '## fingerprint'; systemctl is-active fp6-fingerprint-preserved 2>&1; ls /dev | grep -E 'tee|qseecom|smcinvoke' || true
echo '## charger-mode'; grep -o 'androidboot.mode=[a-z]*' /proc/cmdline 2>/dev/null || $SUDO grep -o 'androidboot.mode=[a-z]*' /proc/cmdline || echo 'no androidboot.mode (normal/RAM boot)'
" > "$out" 2>&1
echo "wrote $out"
