#!/bin/sh
# Run fp6-capture (gst-fp6camera phase 2 CLI) from given store paths, no
# system change. Refuses while the camera is in use.
# usage: fp6-capture-env.sh <libcamera> <gst-fp6camera> [fp6-capture args...]
LC=$1; FP=$2; shift 2
if pgrep -x "gst-launch-1.0|cam|qcam|fp6-capture|fp6-long-exposu|lomiri-camera-a|.lomiri-camera-|fp6camera-selft" >/dev/null; then echo "camera busy"; exit 3; fi
w=$(readlink -f /run/current-system/sw/bin/lomiri-camera-app)
base=$(head -c 20000 "$w" | tr -c '[:print:]' '\n' | grep -o "/nix/store/[^:' ]*/lib/gstreamer-1.0" | grep -v -e libcamera -e fp6camera | sort -u | tr '\n' ':')
export GST_PLUGIN_SYSTEM_PATH_1_0=$FP/lib/gstreamer-1.0:$LC/lib/gstreamer-1.0:$base
export GST_REGISTRY=/tmp/fp6-phase2-registry.bin
export LIBCAMERA_LOG_LEVELS="${LIBCAMERA_LOG_LEVELS:-*:ERROR}"
export GST_DEBUG="${GST_DEBUG:-fp6camera:4}"
export LIBCAMERA_IPA_CONFIG_PATH=$FP/share/libcamera/ipa
exec timeout ${TMO:-300} $FP/bin/fp6-capture "$@"
