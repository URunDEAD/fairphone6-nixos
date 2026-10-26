#!/bin/sh
# Creates a static fake IIO tree with the FP6 sensor names and values read
# live on 2026-10-26 (phone upright at a window), for offline UI previews:
#   FP6_IIO_ROOT=$dir/iio fp6-sensor-tools compass
set -eu
root=$1/iio
dev() { d=$root/iio:device$1; mkdir -p "$d"; echo "$2" > "$d/name"; shift 2
        while [ $# -gt 1 ]; do echo "$2" > "$d/$1"; shift 2; done; }
dev 0 stk36c61 in_illuminance_raw 47 in_illuminance_scale 6.400000 in_proximity_raw 3 \
  in_intensity_red_raw 120 in_intensity_green_raw 210 in_intensity_blue_raw 90 in_intensity_clear_raw 400
dev 1 icm42631-gyro in_anglvel_x_raw -3 in_anglvel_y_raw 2 in_anglvel_z_raw 1 \
  in_anglvel_scale 0.001065264 mount_matrix "-1, 0, 0; 0, 1, 0; 0, 0, -1" \
  in_temp_raw 1528 in_temp_offset 3312 in_temp_scale 7.548309
dev 2 icm42631-accel in_accel_x_raw -15 in_accel_y_raw -2050 in_accel_z_raw -44 \
  in_accel_scale 0.004788403 mount_matrix "-1, 0, 0; 0, 1, 0; 0, 0, -1" \
  in_temp_raw 1526 in_temp_offset 3312 in_temp_scale 7.548309
dev 3 qmc6308 in_magn_x_raw -478 in_magn_y_raw 528 in_magn_z_raw 1787 in_magn_scale 0.001000000 \
  in_mount_matrix "0, -1, 0; 1, 0, 0; 0, 0, 1"
dev 4 spl07-003 in_pressure_input 100.998000000 in_temp_input 33132
echo "$root"
