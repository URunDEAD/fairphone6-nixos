#!/usr/bin/env python3
"""Test-only touch injector for the FP6 touchscreen (ESWIN, /dev/input/event3).

Commands (physical pixels, 1116x2412 portrait panel coordinates):
  tap X Y              short tap
  hold X Y MS          press, stay MS ms, release
  swipe X1 Y1 X2 Y2 MS move in ~16 ms steps over MS ms
  dragpress X1 Y1 X2 Y2 MS HOLD  press, wait HOLD ms, then move over MS ms, release
  wait MS
Usage: touch.py tap 500 1200 wait 300 hold 500 1200 900 ...
"""
from pathlib import Path
import struct
import sys
import time

assert 'ESWIN EPH86XX' in Path('/sys/class/input/event3/device/name').read_text()
args = sys.argv[1:]
f = open('/dev/input/event3', 'wb', buffering=0)


def emit(*events):
    f.write(b''.join(struct.pack('llHHi', 0, 0, *e) for e in (*events, (0, 0, 0))))


def down(x, y):
    emit((3, 47, 0), (3, 57, 77), (3, 53, x), (3, 54, y), (3, 0, x), (3, 1, y), (1, 330, 1))


def move(x, y):
    emit((3, 53, x), (3, 54, y), (3, 0, x), (3, 1, y))


def up():
    emit((3, 57, -1), (1, 330, 0))


def path(x1, y1, x2, y2, ms):
    steps = max(2, int(ms / 16))
    for i in range(1, steps + 1):
        move(round(x1 + (x2 - x1) * i / steps), round(y1 + (y2 - y1) * i / steps))
        time.sleep(ms / 1000 / steps)


i = 0
while i < len(args):
    c = args[i]
    if c == 'tap':
        x, y = int(args[i + 1]), int(args[i + 2]); i += 3
        down(x, y); time.sleep(.05); move(x, y); time.sleep(.05); up()
    elif c == 'hold':
        x, y, ms = int(args[i + 1]), int(args[i + 2]), int(args[i + 3]); i += 4
        down(x, y)
        t = 0
        while t < ms:
            time.sleep(.05); t += 50
            move(x, y)
        up()
    elif c == 'swipe':
        x1, y1, x2, y2, ms = (int(a) for a in args[i + 1:i + 6]); i += 6
        down(x1, y1); path(x1, y1, x2, y2, ms); up()
    elif c == 'dragpress':
        x1, y1, x2, y2, ms, hold = (int(a) for a in args[i + 1:i + 7]); i += 7
        down(x1, y1); time.sleep(hold / 1000); path(x1, y1, x2, y2, ms); time.sleep(.1); up()
    elif c == 'wait':
        time.sleep(int(args[i + 1]) / 1000); i += 2
    else:
        raise SystemExit('bad command ' + c)
    time.sleep(.03)
