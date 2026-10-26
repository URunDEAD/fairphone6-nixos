#!/usr/bin/env python3
"""Type a numeric test login through a short-lived virtual keyboard."""
import fcntl
from pathlib import Path
import struct
import sys
import time

assert 'The Fairphone (Gen. 6)' == Path('/sys/firmware/devicetree/base/model').read_text().rstrip('\0')
text = sys.argv[1]
assert text.isdigit() and len(text) <= 20
with open('/dev/uinput', 'wb', buffering=0) as f:
    fcntl.ioctl(f, 0x40045564, 1)  # UI_SET_EVBIT(EV_KEY)
    for key in range(1, 128):
        fcntl.ioctl(f, 0x40045565, key)  # UI_SET_KEYBIT
    fcntl.ioctl(f, 0x405c5503, struct.pack('HHHH80sI', 3, 1, 1, 1, b'FP6 setup test keyboard', 0))
    fcntl.ioctl(f, 0x5501)  # UI_DEV_CREATE
    try:
        time.sleep(2)
        for key in [11 if c == '0' else int(c) + 1 for c in text] + [28]:
            f.write(struct.pack('llHHi', 0, 0, 1, key, 1) + struct.pack('llHHi', 0, 0, 0, 0, 0))
            time.sleep(.04)
            f.write(struct.pack('llHHi', 0, 0, 1, key, 0) + struct.pack('llHHi', 0, 0, 0, 0, 0))
            time.sleep(.04)
        time.sleep(1)
    finally:
        fcntl.ioctl(f, 0x5502)  # UI_DEV_DESTROY
