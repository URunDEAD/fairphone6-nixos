#!/bin/sh
# READ-ONLY (udev/sysfs reads; monitor-sensor claims the accelerometer via D-Bus).
. "$(dirname "$0")/sensors-common.sh"
phone 'for d in /sys/bus/iio/devices/iio:device*; do n=$(cat $d/name); case $n in icm426*-accel) A=$d;; esac; done
  echo "accel=$A name=$(cat $A/name)"
  udevadm info $A | grep IIO_SENSOR_PROXY_TYPE
  echo "buffer/enable=$(cat $A/buffer/enable)  (must be 0 with the 81-fp6-accel-poll rule)"
  echo "mount_matrix=$(cat $A/mount_matrix)"
  cat $A/in_accel_x_raw $A/in_accel_y_raw $A/in_accel_z_raw 2>&1 | tr "\n" " "; echo "(raw xyz; EBUSY = buffer mode, poll rule missing)"'
echo "Rotate the phone through: portrait (normal), left side down, right side down, upside down, back to portrait."
echo "Expected AccelerometerOrientation: normal -> left-up / right-up (one each) -> bottom-up -> normal."
prompt "Start (25 s capture)"
phone 'timeout 25 monitor-sensor --accel 2>&1' | grep -i -E "orientation|accel" | uniq | tail -20 || true
cat <<'TXT'

Lomiri check (after integrating sensors-lomiri.nix): unlock, open an app that allows
landscape (e.g. lomiri-calculator-app / morph-browser), rotate the phone; the shell
must rotate and touch must map correctly in landscape (tap a button at the rotated
position). Lomiri journal: QT_LOGGING_RULES="qtmir.sensors.debug=true" shows
OrientationSensor::readingChanged on rotation.
TXT
