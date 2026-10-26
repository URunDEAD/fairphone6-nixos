#!/usr/bin/env python3
"""Inject a tap or swipe into the verified FP6 touchscreen for UI testing."""
from pathlib import Path
import struct
import sys
import time

assert 'ESWIN EPH86XX' in Path('/sys/class/input/event3/device/name').read_text()
coords = [int(x) for x in sys.argv[1:]]
assert len(coords) in (2, 4)
assert all(0 <= x < (1116 if i % 2 == 0 else 2484) for i, x in enumerate(coords))
is_tap = len(coords) == 2
if is_tap:
    coords += coords
with open('/dev/input/event3', 'wb', buffering=0) as f:
    def emit(*events):
        f.write(b''.join(struct.pack('llHHi', 0, 0, *event) for event in (*events, (0, 0, 0))))

    emit((3, 47, 0), (3, 57, 42), (3, 53, coords[0]), (3, 54, coords[1]),
         (3, 0, coords[0]), (3, 1, coords[1]), (1, 330, 1))
    # A tap must stay below Maliit's 300 ms long-press threshold.
    steps = 4 if is_tap else 20
    for i in range(1, steps + 1):
        x = round(coords[0] + (coords[2] - coords[0]) * i / steps)
        y = round(coords[1] + (coords[3] - coords[1]) * i / steps)
        emit((3, 53, x), (3, 54, y), (3, 0, x), (3, 1, y))
        time.sleep(.02)
    emit((3, 57, -1), (1, 330, 0))
