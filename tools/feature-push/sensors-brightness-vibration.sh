#!/bin/sh
# CHANGES STATE (brightness writes through D-Bus, one 40 ms vibration). Needs
# fp6-lomiri-powerd (or the old manual bridge) running and hfd-service installed.
. "$(dirname "$0")/sensors-common.sh"
S='busctl --system call com.lomiri.Repowerd /com/lomiri/Repowerd com.lomiri.Repowerd'
phone "$S getBrightnessParams"
echo "(dim min max default autobrightness-supported) — last field must be true with HasAmbientLight"
old=$(phone 'cat /sys/class/backlight/*/brightness')
for v in 40 200 120; do phone "$S setUserBrightness i $v"; sleep 1; echo "logical $v -> raw $(phone 'cat /sys/class/backlight/*/brightness')"; done
prompt "Auto-brightness on for 20 s: cover the sensor, then shine light on it"
phone "$S userAutobrightnessEnable b true"
for i in 1 2 3 4 5 6 7 8 9 10; do echo "raw=$(phone 'cat /sys/class/backlight/*/brightness') lux=$(phone 'busctl --system get-property net.hadess.SensorProxy /net/hadess/SensorProxy net.hadess.SensorProxy LightLevel')"; sleep 2; done
phone "$S userAutobrightnessEnable b false"
echo "PASS: brightness follows light (dark -> low, bright -> high) without flicker."
echo "Restoring raw $old via logical $(( (old*255+2047)/4095 ))"
phone "$S setUserBrightness i $(( (old*255+2047)/4095 ))"
prompt "Vibration: one 40 ms pulse through hfd's USER API (as the phone user)"
phone 'busctl --system call com.lomiri.hfd /com/lomiri/hfd com.lomiri.hfd.Vibrator vibrate i 40'
echo "PASS: call returns, a short buzz is felt. In Lomiri also toggle a switch in System Settings"
echo "(UITK haptic feedback) and check 'journalctl -u hfd-service' shows the request."
