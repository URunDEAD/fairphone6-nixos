#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Drive fp6-screenrecord like Lomiri's ScreenRecorder item does: spawn it with
one end of a socketpair (--fd), send synthetic RGBA frames (32-byte FP6F
header + pixels, CLOCK_MONOTONIC pts), close the socket and wait.

usage: feed-frames.py RECORDER OUT_DIR [--frames N] [--size WxH] [--fps F]
       [--gap-after N SECONDS] [--rotate R] [extra recorder args...]

The frames show a white bar moving left to right over a red/green gradient,
so orientation and motion can be checked in the result. --gap-after N S
pauses S seconds after frame N (a still screen: no frames, like Lomiri when
nothing changes).
"""
import os
import socket
import struct
import subprocess
import sys
import time


def main():
    args = sys.argv[1:]
    recorder, out_dir = args[0], args[1]
    rest = args[2:]
    frames, size, fps, gap, rotate = 60, (90, 160), 30, None, "0"
    extra = []
    i = 0
    while i < len(rest):
        a = rest[i]
        if a == "--frames":
            frames = int(rest[i + 1]); i += 2
        elif a == "--size":
            w, h = rest[i + 1].split("x"); size = (int(w), int(h)); i += 2
        elif a == "--fps":
            fps = int(rest[i + 1]); i += 2
        elif a == "--gap-after":
            gap = (int(rest[i + 1]), float(rest[i + 2])); i += 3
        elif a == "--rotate":
            rotate = rest[i + 1]; i += 2
        else:
            extra.append(a); i += 1
    w, h = size
    parent, child = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
    os.set_inheritable(child.fileno(), True)

    env = dict(os.environ, XDG_VIDEOS_DIR=out_dir)
    proc = subprocess.Popen([recorder, "--fd", str(child.fileno()), "--rotate", rotate, "--fps", str(fps),
                             "--out-dir", out_dir, *extra],
                            pass_fds=(child.fileno(),), env=env)
    child.close()
    base = bytearray(w * h * 4)
    for y in range(h):
        for x in range(w):
            o = (y * w + x) * 4
            base[o:o + 4] = bytes((255 * x // w, 255 * y // h, 40, 255))
    for n in range(frames):
        frame = bytearray(base)
        bar = n * w // frames
        for y in range(h // 4):          # bar only in the top quarter
            for x in range(bar, min(w, bar + max(2, w // 20))):
                o = (y * w + x) * 4
                frame[o:o + 4] = b"\xff\xff\xff\xff"
        header = b"FP6F" + struct.pack("<IIIIIq", 1, w, h, w * 4, 0, time.clock_gettime_ns(time.CLOCK_MONOTONIC))
        parent.sendall(header + frame)
        time.sleep(1 / fps)
        if gap and n == gap[0]:
            time.sleep(gap[1])
    parent.close()
    return proc.wait(timeout=60)


if __name__ == "__main__":
    sys.exit(main())
