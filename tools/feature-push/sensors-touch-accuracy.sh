#!/bin/sh
# READ-ONLY: opens the ESWIN evdev node read-only (no grab) and prints tap coordinates.
. "$(dirname "$0")/sensors-common.sh"
echo "Tap, in order: top-left corner, top-right, bottom-right, bottom-left, exact centre, (30 s)."
echo "Expected (portrait, X 0..1116, Y 0..2484): ~(0,0) (1116,0) (1116,2484) (0,2484) (558,1242)."
prompt "Start"
phone 'python3 - <<PY
import os, struct, time, glob, select
node = None
for e in sorted(glob.glob("/sys/class/input/event*")):
    if open(e + "/device/name").read().strip() == "ESWIN EPH86XX Touchscreen":
        node = "/dev/input/" + os.path.basename(e)
fd = os.open(node, os.O_RDONLY | os.O_NONBLOCK)
S = struct.Struct("llHHi"); x = y = None; down = False; end = time.time() + 30
while time.time() < end:
    if not select.select([fd], [], [], 0.5)[0]: continue
    data = os.read(fd, S.size * 64)
    for o in range(0, len(data), S.size):
        _, _, t, c, v = S.unpack_from(data, o)
        if t == 3 and c == 0x35: x = v
        elif t == 3 and c == 0x36: y = v
        elif t == 1 and c == 330:
            if v == 1: down = True
            elif down: print("tap", x, y, flush=True); down = False
        elif t == 1 and c == 143: print("KEY_WAKEUP", v, flush=True)
PY'
echo "PASS: corners within ~15 px of the panel edges, centre within ~15 px; no axis swap/mirror."
