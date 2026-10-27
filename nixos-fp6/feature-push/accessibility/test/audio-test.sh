#!/bin/sh
# Laptop end-to-end test of mono audio / balance: GSettings (keyfile backend)
# -> fp6-accessibility-audio (the real bridge) -> WirePlumber settings ->
# fp6/a11y-audio.lua -> the stereo output's audioconvert filter graph, on a
# private PipeWire + WirePlumber (a stereo pipe sink "fake_speaker" whose
# output is read back, a mono null sink "fake_earpiece", no hardware). A
# left-only 1 kHz tone is played; the per-channel RMS shows the result.
#   test/audio-test.sh [OUTDIR]
set -u
D=$(dirname "$(readlink -f "$0")")
NP=$(nix-instantiate --eval -E "(import $D/../../../upstream/mobile-nixos/npins).nixpkgs.outPath" | tr -d '"')
nb() { nix-build --no-out-link -E "let p = import $NP {}; in p.$1"; }
PW=$(nb pipewire); WP=$(nb wireplumber); PY=$(nb python3)/bin/python3; GS=$(nb glib.bin)/bin/gsettings
SCHEMA=$(nix-build --no-out-link -E "let p = import $NP {}; in p.callPackage $D/../schema.nix {}")
BRIDGE=$(nix-build --no-out-link -E "let p = import $NP {}; in p.callPackage $D/../fp6-accessibility-audio/package.nix { schema = p.callPackage $D/../schema.nix {}; }")
OUT=${1:-$(mktemp -d)}; mkdir -p "$OUT"
W=$(mktemp -d)
mkdir -p "$W/cfg/pipewire/pipewire.conf.d" "$W/cfg/wireplumber/wireplumber.conf.d" "$W/data/wireplumber/scripts/fp6"
mkfifo "$W/speaker.fifo"
# The stereo "speaker" is a pipe sink: what reaches the device (after the
# node's audioconvert, i.e. after the filter graph) ends up in a FIFO. A null
# sink's monitor would show the converter's input, before the graph.
cat > "$W/cfg/pipewire/pipewire.conf.d/null.conf" <<CONF
context.modules = [
  { name = libpipewire-module-pipe-tunnel args = { tunnel.mode = sink pipe.filename = "$W/speaker.fifo"
      audio.format = S16LE audio.rate = 48000 audio.channels = 2 audio.position = [ FL FR ]
      stream.props = { node.name = "fake_speaker" node.description = "Fake speaker" priority.session = 1000 } } }
]
context.objects = [
  { factory = adapter args = { factory.name = support.null-audio-sink node.name = "fake_earpiece" node.description = "Fake earpiece" media.class = Audio/Sink audio.position = [ MONO ] audio.channels = 1 object.linger = true node.virtual = false priority.session = 900 device.api = "fake" } }
]
CONF
cat > "$W/cfg/wireplumber/wireplumber.conf.d/no-hw.conf" <<'CONF'
wireplumber.profiles = { main = { monitor.alsa = disabled, monitor.bluez = disabled, monitor.v4l2 = disabled, monitor.libcamera = disabled, monitor.alsa-midi = disabled, monitor.bluez-midi = disabled, support.logind = disabled } }
CONF
cp "$D/../audio/wireplumber-fp6-a11y.conf" "$W/cfg/wireplumber/wireplumber.conf.d/60-fp6-a11y.conf"
cp "$D/../audio/fp6-a11y-audio.lua" "$W/data/wireplumber/scripts/fp6/a11y-audio.lua"
R=/run/user/$(id -u)/fp6a11y.$$; mkdir -m700 "$R"
export XDG_RUNTIME_DIR=$R XDG_CONFIG_HOME=$W/cfg XDG_DATA_HOME=$W/data XDG_STATE_HOME=$W/state HOME=$W
export GSETTINGS_BACKEND=keyfile GSETTINGS_SCHEMA_DIR=$(echo $SCHEMA/share/gsettings-schemas/*/glib-2.0/schemas)
export PIPEWIRE_DEBUG=${PIPEWIRE_DEBUG:-2} WIREPLUMBER_DEBUG=s-fp6-a11y:4
$PW/bin/pipewire > "$W/pw.log" 2>&1 & PWP=$!; sleep 1
$WP/bin/wireplumber > "$W/wp.log" 2>&1 & WPP=$!; sleep 2
$BRIDGE/bin/fp6-accessibility-audio run > "$W/bridge.log" 2>&1 & BR=$!
$PY - "$W/tone.wav" <<'PYEOF'
import sys, wave, struct, math
w = wave.open(sys.argv[1], "wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
frames = b"".join(struct.pack("<hh", int(16000 * math.sin(2 * math.pi * 1000 * i / 48000)), 0) for i in range(48000 * 3))
w.writeframes(frames); w.close()
PYEOF
# Reads the FIFO continuously; each measurement takes the newest 1.2 s.
$PY - "$W/speaker.fifo" "$W/speaker.raw" <<'PYEOF' &
import sys
with open(sys.argv[1], "rb") as f, open(sys.argv[2], "wb", buffering=0) as o:
    while True:
        b = f.read(19200)
        if not b: break
        o.write(b)
PYEOF
READER=$!
measure() {   # label
  $PW/bin/pw-play --target fake_speaker "$W/tone.wav" & PLAY=$!
  sleep 1.5
  $PY - "$W/speaker.raw" "$1" <<'PYEOF'
import sys, struct, math, os
data = open(sys.argv[1], "rb").read()
d = data[-48000 * 4:]          # last second (stereo s16)
d = d[:len(d) // 4 * 4]
s = struct.unpack("<%dh" % (len(d) // 2), d)
rms = lambda v: math.sqrt(sum(x * x for x in v) / max(1, len(v)))
ref = 16000 / math.sqrt(2)
best = (0.0, 0.0)               # loudest 0.1 s window (skips gaps)
for i in range(0, len(s) - 9600, 9600):
    w = s[i:i + 9600]; a, b = rms(w[0::2]), rms(w[1::2])
    if a + b > sum(best): best = (a, b)
print("%-28s L %.3f  R %.3f  (x the tone's RMS)" % (sys.argv[2], best[0] / ref, best[1] / ref))
PYEOF
  kill $PLAY 2>/dev/null; wait $PLAY 2>/dev/null; sleep 0.3
}
set_gs() { $GS set com.fairphone6.accessibility "$1" "$2"; sleep 0.8; }
{
echo "== stereo sink, left-only tone"
measure "default (no graph)"
set_gs mono-audio true;          measure "mono"
set_gs audio-balance 0.5;        measure "mono + balance 0.5 (right)"
set_gs mono-audio false;         measure "balance 0.5 (right)"
set_gs audio-balance -0.5;       measure "balance -0.5 (left)"
set_gs audio-balance 1.0;        measure "balance 1 (right only)"
set_gs audio-balance 0.0;        measure "back to neutral"
echo "== WirePlumber restart keeps the setting (persistent-sm-settings)"
set_gs mono-audio true
kill $WPP; sleep 0.5; $WP/bin/wireplumber > "$W/wp2.log" 2>&1 & WPP=$!; sleep 2.5
measure "mono after WP restart"
set_gs mono-audio false
echo "== bridge log"; cat "$W/bridge.log"
echo "== script log"
cat "$W/wp.log" "$W/wp2.log" | grep -i "fp6 accessibility audio" | sed 's/.*fp6 accessibility audio/fp6 accessibility audio/' | cut -c1-110
echo "== mono earpiece untouched:"; grep -c "fake_earpiece" "$W/wp.log" | sed 's/^/log lines naming fake_earpiece: /'
grep -iE "error|warn" "$W/pw.log" "$W/wp.log" "$W/wp2.log" | grep -v "rtkit\|RTKit\|dbus\|D-Bus\|portal\|libcamera\|bluez\|logind\|v4l2\|alsa\|jack\|could not set nice\|realtime\|snap\|reserve\|org\.freedesktop" | head -10
} 2>&1 | tee "$OUT/audio-test.txt"
if [ -n "${KEEP:-}" ]; then echo "kept running: XDG_RUNTIME_DIR=$R"; exit 0; fi
kill $READER $BR $WPP $PWP 2>/dev/null; sleep 0.5; rm -rf "$R"
