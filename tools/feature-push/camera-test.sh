#!/usr/bin/env bash
# FP6 camera live test (lomiri-camera-app stack) -- run by the PARENT only.
#
# Requires the phone booted into a generation built from
#   nixos-fp6/feature-push/camera/trial-lomiri-camera-fp6.nix
# (or any system with hardware.fp6LomiriCamera.enable = true).
#
# Usage: [FP6_TEST_PLUGIN=/tmp/dir] [STRESS_N=n] tools/feature-push/camera-test.sh [STEP...]
#   FP6_TEST_PLUGIN: phone-side prefix of a candidate gst-fp6camera build (copied
#   under /tmp) used instead of the installed one
#   steps (default: preflight selftest photo flash video switch collect)
#     preflight  identity, cam -l, camera users, LED state, SMMU fault baseline
#     selftest   offline plugin self test ON the phone (videotestsrc, temp LED)
#     photo      one still per sensor through camerabin: primary, secondary, wide
#     flash      one primary still with the bounded flash pulse (+ hard cutoff)
#     video      5 s MP4 (x264enc, Iris encoder hidden) on primary and secondary
#     venc       5 s MP4 allowing the Iris V4L2 m2m encoder (v4l2h264enc)
#     switch     primary -> secondary -> primary -> secondary stills back to back
#     stress     STRESS_N (default 6) alternating front/back stills, failure tally
#     collect    copy outputs + kernel log delta into results/
#     app        print the manual GUI acceptance checklist
#
# Every camera session is bounded (timeout); the LED is forced to 0 on exit.
set -u
here=$(cd "$(dirname "$0")/../.." && pwd)
ssh="$here/tools/phone-ssh"
stamp=$(date +%Y%m%dT%H%M%S)
out="$here/results/feature-push-20261024/camera/live-$stamp"
remote=/tmp/fp6cam-test
led=/sys/class/leds/white:flash/brightness
wide=/base/soc@0/cci@ac15000/i2c-bus@1/camera@36
sudo_r='SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ""'
mkdir -p "$out"
log() { printf '%s %s\n' "$(date +%T)" "$*" | tee -a "$out/run.log"; }
r() { "$ssh" "$@" 2>&1 | tee -a "$out/run.log"; return "${PIPESTATUS[0]}"; }

# Environment identical to the wrapped lomiri-camera-app (plugin paths, config).
envsetup="FP6_TEST_PLUGIN='${FP6_TEST_PLUGIN:-}'
"$(cat <<'EOF'
set -u
w=$(readlink -f "$(command -v lomiri-camera-app)") || { echo "NO lomiri-camera-app"; exit 3; }
d=$(dirname "$w")
paths=$(cat "$d"/lomiri-camera-app "$d"/.lomiri-camera-app-wrapped 2>/dev/null \
  | grep -ao '/nix/store/[a-z0-9]*-[^/:"'"'"' ]*/lib/gstreamer-1.0' | sort -u | tr '\n' ':')
case "$paths" in *gst-fp6camera*) ;; *) echo "wrapper has no gst-fp6camera (not the FP6 build)"; exit 3;; esac
selftest=$(printf '%s' "$paths" | tr ':' '\n' | grep gst-fp6camera | head -n1)/../../libexec/fp6camera-selftest
if [ -n "${FP6_TEST_PLUGIN:-}" ]; then   # candidate plugin copied under /tmp
  paths="$FP6_TEST_PLUGIN/lib/gstreamer-1.0:$(printf '%s' "$paths" | tr ':' '\n' | grep -v gst-fp6camera | tr '\n' ':')"
  selftest=$FP6_TEST_PLUGIN/libexec/fp6camera-selftest
fi
mkdir -p /tmp/fp6cam-test
export GST_PLUGIN_SYSTEM_PATH_1_0="${paths%:}" GST_REGISTRY=/tmp/fp6cam-test/registry.bin
export FP6_CAMERA_CONFIG=/etc/fp6-camera/camera.conf
export LIBCAMERA_IPA_CONFIG_PATH="$(dirname "$selftest")/../share/libcamera/ipa"
export GST_PLUGIN_FEATURE_RANK=v4l2h264enc:NONE,v4l2h265enc:NONE,v4l2vp8enc:NONE,v4l2vp9enc:NONE
led_off() { echo 0 > /sys/class/leds/white:flash/brightness 2>/dev/null || true; }
trap led_off EXIT
EOF
)
run_remote() { r "$envsetup
$1"; }

step_preflight() {
  log "== preflight"
  r 'uname -r; readlink -f /run/current-system; cat /etc/fp6-camera/camera.conf || echo "NO /etc/fp6-camera/camera.conf"'
  r 'cam -l 2>/dev/null | tail -4'
  r 'ls -l /sys/class/leds/white:flash/brightness; cat /sys/class/leds/white:flash/brightness'
  r 'pgrep -a -f "lomiri-camera-app|snapshot|gst-launch|cam " || echo "no other camera users"'
  r "$sudo_r dmesg | grep -ciE 'smmu|iommu fault|camss.*error'" > "$out/smmu-before.txt" || true
  r "$sudo_r dmesg | tail -n 1" > "$out/dmesg-last-before.txt" || true
}

step_selftest() {
  log "== selftest (no camera, temp LED file)"
  run_remote 'cd /tmp/fp6cam-test && echo 0 > led && printf "[flash]\nled-brightness-path=/tmp/fp6cam-test/led\nbrightness=160\npreflash-ms=200\ncapture-hold-max-ms=800\n" > selftest.conf &&
    FP6_CAMERA_CONFIG=$PWD/selftest.conf FP6_SELFTEST_LED=$PWD/led FP6_SELFTEST_IMG=$PWD/selftest.jpg timeout 60 "$selftest"'
}

photo() { # device name [--flash]
  run_remote "cd /tmp/fp6cam-test && timeout 40 \"\$selftest\" --photo '$1' /tmp/fp6cam-test/photo-$2.jpg ${3:-} && ls -l /tmp/fp6cam-test/photo-$2.jpg"
}

step_photo() {
  log "== photo: primary (IMX896 main), secondary (S5KKD1 front), wide (OV13B10 by id)"
  photo primary primary
  photo secondary secondary
  photo "$wide" wide
}

step_flash() {
  log "== flash: bounded pulse (plugin bound ~1.7 s, independent cutoff at 4 s)"
  # independent hard cutoff in the same remote shell (video group may write
  # brightness), plus the EXIT trap from envsetup
  run_remote "( sleep 4; echo 0 > $led ) </dev/null >/dev/null 2>&1 &
    cd /tmp/fp6cam-test && timeout 40 \"\$selftest\" --photo primary /tmp/fp6cam-test/photo-primary-flash.jpg --flash;
    rc=\$?; echo led-after=\$(cat $led); exit \$rc"
}

video() { # device name [allow-hw]
  local rank='' ; [ "${3:-}" = hw ] && rank='unset GST_PLUGIN_FEATURE_RANK;'
  run_remote "cd /tmp/fp6cam-test && $rank timeout 60 \"\$selftest\" --video '$1' /tmp/fp6cam-test/video-$2.mp4 5 1440x1080 && ls -l /tmp/fp6cam-test/video-$2.mp4 &&
    { command -v gst-discoverer-1.0 >/dev/null && gst-discoverer-1.0 /tmp/fp6cam-test/video-$2.mp4 | grep -E 'Duration|video|audio|Width|Height|Frame rate' || true; }"
}

step_video() {
  log "== video: 5 s 1440x1080 MP4, software x264enc"
  video primary primary
  video secondary secondary
}

step_venc() {
  log "== venc: Iris V4L2 m2m encoder allowed (watch for 'element v4l2h264enc')"
  run_remote 'unset GST_PLUGIN_FEATURE_RANK; gst-inspect-1.0 v4l2h264enc 2>&1 | head -n 20'
  video primary primary-hw hw
}

step_switch() {
  log "== switch: back/front reopen cycle"
  for i in 1 2; do
    photo primary "switch-$i-back" || log "switch $i back FAILED"
    photo secondary "switch-$i-front" || log "switch $i front FAILED"
  done
}

step_stress() { # N alternating front/back stills; reports failures only
  local n=${STRESS_N:-6}
  log "== stress: $n x (secondary, primary) stills"
  run_remote "cd /tmp/fp6cam-test; ok=0; bad=0
    for i in \$(seq $n); do for d in secondary primary; do
      if timeout 30 \"\$selftest\" --photo \$d /tmp/fp6cam-test/stress.jpg > /tmp/fp6cam-test/stress-\$i-\$d.log 2>&1; then ok=\$((ok+1));
      else bad=\$((bad+1)); echo \"FAIL cycle \$i \$d\"; grep -E 'configuring|Input|Best focus|Debayer|FAIL|ERROR|error' /tmp/fp6cam-test/stress-\$i-\$d.log | tail -n 8; fi
    done; done; echo stress ok=\$ok bad=\$bad"
}

step_collect() {
  log "== collect"
  r "echo 0 > $led; cat $led"
  for f in $("$ssh" 'ls /tmp/fp6cam-test/*.jpg /tmp/fp6cam-test/*.mp4 2>/dev/null'); do
    "$ssh" "cat $f" > "$out/$(basename "$f")"
  done
  r "$sudo_r dmesg | grep -ciE 'smmu|iommu fault|camss.*error'" > "$out/smmu-after.txt" || true
  r "$sudo_r dmesg | tail -n 200" > "$out/dmesg-tail-after.txt" || true
  ls -l "$out" | tee -a "$out/run.log"
  command -v ffprobe >/dev/null && for v in "$out"/*.mp4; do
    [ -e "$v" ] && ffprobe -v error -show_entries stream=codec_name,width,height,nb_frames,avg_frame_rate \
      -show_entries format=duration "$v" | tee -a "$out/run.log"
  done
  log "results in $out"
}

step_app() {
  cat <<'EOF' | tee -a "$out/run.log"
Manual GUI acceptance (Lomiri session):
 1. Open Camera. Back preview appears within ~3 s, upright in portrait.
    If rotated: set hardware.fp6LomiriCamera.primary.mountAngle (90/270) -- or
    edit /etc/fp6-camera/camera.conf temporarily -- and relaunch.
 2. Take a photo (flash Off). Gallery shows it upright.
 3. Flash -> On, take a photo: LED lights ~0.4 s before and during the shot, then off.
 4. Switch camera: front preview appears; take a photo; check orientation/mirroring
    (secondary.mountAngle default 270 is unverified).
 5. Switch back and forth 5x: no hang, no black preview.
 6. Video mode, record ~10 s on back and front; play back in Media Player.
 7. Video mode + flash (video light) on: LED stays on in video mode, off when
    leaving video mode or closing the app.
 8. Close the app: LED 0, `cam -l` still lists 3 cameras, no new SMMU faults.
EOF
}

steps=${*:-preflight selftest photo flash video switch stress collect}
trap '"$ssh" "echo 0 > $led" >/dev/null 2>&1' EXIT
for s in $steps; do "step_$s" || log "step $s FAILED (rc=$?)"; done
