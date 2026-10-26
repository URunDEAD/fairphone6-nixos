"""agps-ttff: time-to-first-fix test for the FP6 modem GNSS (run as root).

  agps-ttff.py AGPS_DIR [--cold] [--assist time,xtra,position] [--config F]
               [--max SECONDS]

--cold     LOC Delete Assistance Data (delete all) first: a true cold start
           (ephemeris, almanac, time, position all forgotten by the engine).
--assist   inject the given assistance with fp6-agps before starting.
Prints TTFF, horizontal uncertainty and satellite counts. Never prints
coordinates.
"""
import importlib.util
import struct
import sys
import time

agps_dir = sys.argv[1]
spec = importlib.util.spec_from_file_location("fp6agps", agps_dir + "/fp6-agps.py")
A = importlib.util.module_from_spec(spec)
spec.loader.exec_module(A)

args = sys.argv[2:]
cold = "--cold" in args
assist = []
cfgpath = "/etc/fp6-agps.json"
maxs = 300
for i, a in enumerate(args):
    if a == "--assist":
        assist = args[i + 1].split(",")
    elif a == "--config":
        cfgpath = args[i + 1]
    elif a == "--max":
        maxs = int(args[i + 1])
cfg = A.load_config(cfgpath)

loc = A.Loc()
loc.register(0x01 | 0x02 | A.EVT_INJECT_ORBITS_REQ)
if cold:
    err, _ = loc.call(0x44, A.tlv(1, "B", 1))
    print(f"delete assistance data: {err or 'ok'}", flush=True)
    time.sleep(2)
if "time" in assist:
    A.inject_time(loc, cfg)
if "xtra" in assist:
    A.inject_xtra(loc, cfg)
if "position" in assist:
    A.inject_position(loc, cfg)
val = A.orbits_validity(loc)
print(f"xtra validity: {val.get('durationHours')} h", flush=True)

r = loc.request(0x22, A.tlv(1, "B", 7) + A.tlv(0x10, "I", 1) + A.tlv(0x12, "I", 1) + A.tlv(0x13, "I", 1000))
t0 = time.time()
print(f"start: {r}", flush=True)
tracked = 0
last_print = 0
ttff = None
try:
    while time.time() - t0 < maxs:
        p = loc.recv(2)
        if not p or p[0] != 4:
            continue
        m, tl = p[2], p[3]
        if m == 0x25:
            b = tl.get(0x11) or tl.get(0x10)
            if b and b[0]:
                n = b[0]
                ent = (len(b) - 1) // n
                snrs = []
                for i in range(n):
                    e = b[1 + i * ent: 1 + (i + 1) * ent]
                    snrs.append(struct.unpack_from("<f", e, 24)[0])
                tracked = sum(1 for s in snrs if s > 0)
                best = sorted(snrs, reverse=True)[:4]
                if time.time() - last_print > 15:
                    last_print = time.time()
                    print(f"  t={time.time() - t0:5.0f}s in view {n}, tracked {tracked}, "
                          f"best SNR {' '.join(f'{s:.0f}' for s in best)}", flush=True)
        elif m == 0x24:
            status = struct.unpack("<I", tl[1][:4])[0] if 1 in tl else 1
            if status == 0 and 0x10 in tl:
                unc = struct.unpack("<f", tl[0x12])[0] if 0x12 in tl else -1
                used = tl[0x2C][0] if 0x2C in tl and tl[0x2C] else -1
                ttff = time.time() - t0
                print(f"FIX after {ttff:.1f} s: horizontal uncertainty {unc:.0f} m, "
                      f"{used} SVs used, {tracked} tracked", flush=True)
                break
finally:
    loc.request(0x23, A.tlv(1, "B", 7), timeout=3)
if ttff is None:
    print(f"NO FIX within {maxs} s ({tracked} tracked at the end)")
