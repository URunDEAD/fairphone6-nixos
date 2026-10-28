#!/usr/bin/env bash
# Audio round of the lock sounds on the laptop, inside fp6-ls-env:
# a private D-Bus + PipeWire + WirePlumber + pipewire-pulse with a null sink.
#  * the preloaded SoundEffect stream carries media.role "event" (PipeWire's
#    event/system-sounds role) while it is loaded, and is gone when released;
#  * onset: the time from play() to the first sound at the sink monitor,
#    for the preloaded SoundEffect and for the cold Audio fallback.
# OUTDIR (arg 1): audio-e2e.json with the measurements.
set -eu
here=$(dirname "$(readlink -f "$0")")
out=$(readlink -f "$1")
if [ "${2:-}" != "--inner" ]; then
  exec "$FP6_DBUS_RUN_SESSION" -- bash "$0" "$out" --inner
fi
w=$(mktemp -d); trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$w"' EXIT
export XDG_RUNTIME_DIR=$w/run HOME=$w; mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
export PIPEWIRE_RUNTIME_DIR=$XDG_RUNTIME_DIR PULSE_RUNTIME_PATH=$XDG_RUNTIME_DIR/pulse
export PATH=$FP6_PIPEWIRE/bin:$FP6_WIREPLUMBER/bin:$FP6_PULSE/bin:$PATH
export SPA_PLUGIN_DIR=$FP6_PIPEWIRE/lib/spa-0.2 PIPEWIRE_MODULE_DIR=$FP6_PIPEWIRE/lib/pipewire-0.3
pipewire > "$w/pw.log" 2>&1 &
sleep 0.5
wireplumber > "$w/wp.log" 2>&1 &
pipewire-pulse > "$w/pp.log" 2>&1 &
for _ in $(seq 50); do pactl info >/dev/null 2>&1 && break; sleep 0.1; done
pactl load-module module-null-sink sink_name=fp6null rate=48000 channels=2 >/dev/null
pactl set-default-sink fp6null
sleep 0.5

cp "$FP6_LS_LOMIRI_QML"/Components/Fp6LockSoundPlayer.qml "$FP6_LS_LOMIRI_QML"/Components/Fp6LockSoundCatalog.js "$here/AudioProbe.qml" "$w/"
chmod u+w "$w"/*
cp "$FP6_LS_RINGTONE" "$w/custom.ogg"
printf 'var wav = "%s";\nvar ogg = "%s";\n' "$FP6_LS_BUNDLE/pop-down.wav" "$w/custom.ogg" > "$w/Probe.js"

# Sink monitor recorder with arrival timestamps (mono s16 48 kHz).
"$FP6_PYTHON" - "$w" <<'PY' &
import subprocess, sys, time, json
w = sys.argv[1]
p = subprocess.Popen(["parec", "-d", "fp6null.monitor", "--raw", "--format=s16le", "--rate=48000",
                      "--channels=1", "--latency-msec=5"], stdout=subprocess.PIPE)
chunks = []
end = time.time() + 7.5
while time.time() < end:
    b = p.stdout.read1(960)
    if b: chunks.append((time.time(), len(b)))
    with open(w + "/rec.raw", "ab") as f: f.write(b)
p.kill()
json.dump(chunks, open(w + "/rec.json", "w"))
PY
rec=$!
sleep 0.3
QT_QPA_PLATFORM=offscreen timeout 30 "$FP6_QMLSCENE" "$w/AudioProbe.qml" > "$w/qml.log" 2>&1 &
qml=$!
roles=""
for _ in $(seq 20); do
  sleep 0.2
  r=$(pactl list sink-inputs 2>/dev/null | grep -E 'media.role|application.name|Corked' | tr -s ' \t' ' ' | paste -sd ';' -)
  [ -n "$r" ] && roles="$roles|$r"
done
wait $qml || true
sleep 1
after=$(pactl list sink-inputs short 2>/dev/null | wc -l)
wait $rec || true
grep -v "PROBE\|^$\|style override\|Available styles" "$w/qml.log" | head -20 || true
grep PROBE "$w/qml.log" || { cat "$w/qml.log"; exit 1; }

"$FP6_PYTHON" - "$w" "$out" "$roles" "$after" <<'PY'
import json, struct, sys
w, out, roles, after = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
raw = open(w + "/rec.raw", "rb").read()
chunks = json.load(open(w + "/rec.json"))
# wall-clock time of each sample: chunk arrival minus the samples still to come in it
times = []
for t, n in chunks:
    k = n // 2
    times += [t - (k - 1 - i) / 48000 for i in range(k)]
samples = struct.unpack("<%dh" % (len(raw) // 2), raw[: len(raw) // 2 * 2])
plays = []
for line in open(w + "/qml.log"):
    if "PROBE play" in line:
        f = line.split()
        i = f.index("play")
        plays.append((f[i + 1], int(f[i + 2]) / 1000.0, f[i + 3]))
res = {"plays": [], "stream_props": roles, "streams_after_release": after}
ok = True
for kind, t, backend in plays:
    onset = None
    for idx in range(min(len(times), len(samples))):
        if times[idx] >= t - 0.005 and abs(samples[idx]) > 300:
            onset = (times[idx] - t) * 1000
            break
    res["plays"].append({"kind": kind, "backend": backend, "onset_ms": None if onset is None else round(onset, 1)})
    print(f"latency {kind} ({backend}): " + ("no sound found" if onset is None else f"{onset:.0f} ms from play() to the sink monitor"))
    if onset is None: ok = False
role_ok = "media.role = \"event\"" in roles
print("media.role event on the SoundEffect stream:", "PASS" if role_ok else "FAIL", roles[:300])
print("released stream gone:", "PASS" if after == 0 else f"FAIL ({after} left)")
eff = [p["onset_ms"] for p in res["plays"] if p["kind"] == "effect" and p["onset_ms"] is not None]
if eff and max(eff) > 120: ok = False; print("FAIL effect onset above 120 ms")
json.dump(res, open(out + "/audio-e2e.json", "w"), indent=1)
sys.exit(0 if ok and role_ok and after == 0 else 1)
PY
