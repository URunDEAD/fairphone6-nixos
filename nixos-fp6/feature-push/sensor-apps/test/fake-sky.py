#!/usr/bin/env python3
"""Serves one static sky JSON line per second on a Unix socket (preview only)."""
import json, os, socket, sys, threading, time
path = sys.argv[1]
sats = [dict(c="GPS", prn=p, el=e, az=a, snr=s) for p, e, a, s in
        [(2, 62, 40, 41), (5, 35, 120, 33), (12, 18, 300, 22), (25, 70, 220, 44), (29, 8, 170, None)]]
sats += [dict(c="Galileo", prn=p, el=e, az=a, snr=s) for p, e, a, s in [(4, 50, 80, 38), (11, 25, 260, 27)]]
sats += [dict(c="GLONASS", prn=p, el=e, az=a, snr=s) for p, e, a, s in [(68, 40, 350, 30), (77, 12, 200, None)]]
sats += [dict(c="BeiDou", prn=p, el=e, az=a, snr=s) for p, e, a, s in [(23, 55, 150, 36)]]
line = json.dumps(dict(sats=sats, visible=len(sats), tracked=sum(1 for s in sats if s["snr"]), quality=1, used=7, hdop=0.9, alt=True)) + "\n"
try:
    os.unlink(path)
except FileNotFoundError:
    pass
srv = socket.socket(socket.AF_UNIX)
srv.bind(path)
srv.listen(4)
def serve(c):
    try:
        while True:
            c.sendall(line.encode()); time.sleep(1)
    except OSError:
        pass
while True:
    c, _ = srv.accept()
    threading.Thread(target=serve, args=(c,), daemon=True).start()
