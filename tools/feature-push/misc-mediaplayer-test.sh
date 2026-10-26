#!/bin/sh
# READ-ONLY (default): confirms the Iris V4L2 stateful decoders that
# lomiri-mediaplayer-app's GStreamer will pick, and that the phone user can
# open the decoder node. --play FILE (MANUAL, Lomiri session visible): starts
# the fp6 wrapper with GST_DEBUG decoder-selection logging for 25 s, then
# stops it; reversible (no config change). FILE must be a non-private clip.
. "$(dirname "$0")/misc-common.sh"
good=$(ph 'ls -d /nix/store/*-gst-plugins-good-1.28*[0-9] 2>/dev/null | head -1')
ph "GST_PLUGIN_SYSTEM_PATH_1_0=$good/lib/gstreamer-1.0 timeout 20 gst-inspect-1.0 v4l2h264dec 2>/dev/null | grep -E 'Rank|device|Long-name' ; for v in /sys/class/video4linux/video*; do case \$(cat \$v/name) in qcom-iris-decoder) d=/dev/\${v##*/}; ls -l \$d; test -r \$d -a -w \$d && echo user-can-open=yes || echo user-can-open=no;; esac; done"
if [ "${1:-}" = --play ]; then
  f=${2:?file on phone}
  ph "command -v lomiri-mediaplayer-app >/dev/null || { echo 'mediaplayer not installed in this generation'; exit 0; }
      systemd-run --user --collect -u fp6-mp-hwdecode-test -E GST_DEBUG='v4l2*:4,decodebin*:3,playbin*:3' -p RuntimeMaxSec=25 lomiri-mediaplayer-app '$f'; sleep 28
      journalctl --user -u fp6-mp-hwdecode-test --no-pager | grep -E 'v4l2h26|v4l2vp9|avdec|openh264|created element|ERROR' | head -20"
fi
