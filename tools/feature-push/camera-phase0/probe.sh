#!/bin/sh
# Phase 0 camera probe (runs ON the phone as user). Captures the last frames of
# a fixed-length fp6libcamerasrc run as raw RGB, plus the libcamera IPA log.
# usage: probe.sh DEVICE SECONDS OUTDIR [WxH]
#  env: PROBE_GST_PLUGINS (colon list of plugin dirs, default: from the app wrapper)
#       PROBE_IPA_CONFIG  (LIBCAMERA_IPA_CONFIG_PATH), PROBE_CONF (camera.conf)
#       PROBE_LOG (libcamera log levels, default "*:WARN,IPASoft*:DEBUG,SoftwareIsp:INFO")
set -eu
dev=$1; secs=$2; out=$3; size=${4:-}
if pgrep -f lomiri-camera-app >/dev/null; then echo "camera app running; abort" >&2; exit 3; fi
mkdir -p "$out"; rm -f "$out"/f*.rgb
w=$(readlink -f /run/current-system/sw/bin/lomiri-camera-app)
plug=${PROBE_GST_PLUGINS:-$(head -c 20000 "$w" | tr -c '[:print:]' '\n' | grep -o "/nix/store/[^:' ]*/lib/gstreamer-1.0" | sort -u | tr '\n' ':')}
ipa=${PROBE_IPA_CONFIG:-$(head -c 20000 "$w" | tr -c '[:print:]' '\n' | grep -o "/nix/store/[^:' ]*/share/libcamera/ipa" | sort -u | head -1)}
conf=${PROBE_CONF:-/etc/fp6-camera/camera.conf}
caps=""
[ -n "$size" ] && caps="! video/x-raw,width=${size%x*},height=${size#*x}"
export GST_PLUGIN_SYSTEM_PATH_1_0="$plug" GST_REGISTRY="$out/registry.bin" \
  LIBCAMERA_IPA_CONFIG_PATH="$ipa" FP6_CAMERA_CONFIG="$conf" \
  LIBCAMERA_LOG_LEVELS="${PROBE_LOG:-*:WARN,IPASoft*:DEBUG,SoftwareIsp:INFO}" \
  GST_DEBUG="${PROBE_GST_DEBUG:-fp6*:4}"
echo "plugins=$plug ipa=$ipa conf=$conf" > "$out/env.txt"
timeout -s INT "$secs" gst-launch-1.0 -e fp6libcamerasrc device="$dev" $caps ! \
  videoconvert ! video/x-raw,format=RGB ! \
  multifilesink location="$out/f%05d.rgb" max-files=3 > "$out/gst.log" 2> "$out/lc.log" || true
ls "$out"/f*.rgb | tail -1 > "$out/last.txt"
gst=$(grep -o "stream [0-9]*x[0-9]*" "$out/lc.log" | tail -1 || true)
echo "done $gst $(cat "$out/last.txt")"
