#!/usr/bin/env python3
"""Synthetic sensor/audio sources for the batch-2 laptop previews.

  fake-signals.py accel-walk IIO_DEV_DIR      walking at 1.8 steps/s (upright phone)
  fake-signals.py accel-vibe IIO_DEV_DIR      phone flat, 8 Hz vibration + 3 Hz component
  fake-signals.py magn-metal IIO_DEV_DIR      field with a passing steel object
  fake-signals.py audio FREQ                  raw s16 mono 48 kHz sine+harmonics on stdout (pw-record stand-in)
  fake-signals.py nmea FILE                   writes 120 s of synthetic NMEA (RMC/GGA) starting now

Everything is generated; no recordings, no real positions (the track runs
along the equator from 0 N 0 E)."""
import math, os, random, sys, time

_files = {}

def write(path, value):
    # In place, fixed width, one write: the app keeps the attribute open and
    # re-reads it with pread (like sysfs), so the file must not be replaced.
    f = _files.get(path)
    if f is None:
        f = _files[path] = os.open(path, os.O_WRONLY | os.O_CREAT)
        os.ftruncate(f, 0)
    os.pwrite(f, f"{value:>10}\n".encode(), 0)

def loop(fn, rate=50):
    t0 = time.time()
    while True:
        t = time.time() - t0
        fn(t)
        time.sleep(1 / rate)

def accel(dev, fn):
    scale = 0.004788403
    def step(t):
        x, y, z = fn(t)          # Android frame, m/s^2
        # Android -> proxy frame (-x, -y, z) -> raw via mount "-1,0,0;0,1,0;0,0,-1"
        px, py, pz = -x, -y, z
        rx, ry, rz = -px, py, -pz
        for a, v in zip("xyz", (rx, ry, rz)):
            write(f"{dev}/in_accel_{a}_raw", int(round(v / scale)))
    loop(step)

def main():
    mode = sys.argv[1]
    rnd = random.Random(1)
    if mode == "accel-walk":
        def f(t):
            b = 2.2 * math.sin(2 * math.pi * 1.8 * t) + 0.6 * math.sin(4 * math.pi * 1.8 * t + 0.7)
            return (0.3 * math.sin(math.pi * 1.8 * t) + rnd.gauss(0, 0.1), 9.81 + b + rnd.gauss(0, 0.1), 0.4 + rnd.gauss(0, 0.1))
        accel(sys.argv[2], f)
    elif mode == "accel-vibe":
        def f(t):
            v = 0.35 * math.sin(2 * math.pi * 8 * t) + 0.12 * math.sin(2 * math.pi * 3 * t)
            env = 0.6 + 0.4 * math.sin(2 * math.pi * 0.2 * t)
            return (rnd.gauss(0, 0.01), rnd.gauss(0, 0.01), 9.81 + env * v + rnd.gauss(0, 0.01))
        accel(sys.argv[2], f)
    elif mode == "magn-metal":
        dev = sys.argv[2]
        base = (-478, 528, 1787)
        def step(t):
            # a screw passes under the phone every 10 s (bump of up to ~70 uT)
            k = math.exp(-((t % 10.0) - 7.0) ** 2 / 0.5)
            for i, a in enumerate("xyz"):
                v = base[i] + (700 * k if a == "z" else 120 * k) + rnd.gauss(0, 3)
                write(f"{dev}/in_magn_{a}_raw", int(round(v)))
        loop(step, 25)
    elif mode == "audio":
        f0 = float(sys.argv[2])
        fs, n = 48000, 960
        out = sys.stdout.buffer
        i = 0
        t0 = time.time()
        while True:
            buf = bytearray()
            for k in range(n):
                t = (i + k) / fs
                v = 0.25 * math.sin(2 * math.pi * f0 * t) + 0.12 * math.sin(4 * math.pi * f0 * t + 1) + 0.05 * math.sin(6 * math.pi * f0 * t)
                s = int(max(-1, min(1, v + rnd.gauss(0, 0.002))) * 32767)
                buf += s.to_bytes(2, "little", signed=True)
            out.write(buf)
            out.flush()
            i += n
            time.sleep(max(0, t0 + i / fs - time.time()))
    elif mode == "nmea":
        def cks(s):
            c = 0
            for ch in s:
                c ^= ord(ch)
            return f"${s}*{c:02X}"
        now = time.time() + 2
        lon = 0.0
        lines = []
        for k in range(120):
            t = now + k
            v = 13.0 + 3.0 * math.sin(k / 15)  # m/s
            lon += v / 111195.08
            g = time.gmtime(t)
            hms = time.strftime("%H%M%S", g) + ".00"
            dmy = time.strftime("%d%m%y", g)
            lond = int(lon)
            lonm = (lon - lond) * 60
            lon_s = f"{lond:03d}{lonm:07.4f}"
            kn = v * 1.9438445
            lines.append(cks(f"GPGGA,{hms},0000.0000,N,{lon_s},E,1,12,0.8,100.0,M,0.0,M,,"))
            lines.append(cks(f"GPRMC,{hms},A,0000.0000,N,{lon_s},E,{kn:.2f},90.0,{dmy},,,A"))
        with open(sys.argv[2], "w") as f:
            f.write("\r\n".join(lines) + "\r\n")
    else:
        sys.exit(__doc__)

if __name__ == "__main__":
    main()
