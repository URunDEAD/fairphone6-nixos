"""fp6-loc-nmea: NMEA from the FP6 modem GNSS engine (raw QMI LOC over QRTR).

Why not libqmi: on this modem firmware (MPSS.DE.3.1.4 MILOS) LOC
RegisterEvents is rejected with InvalidArgument unless the request carries
the optional clientStrId (0x10) and clientType (0x11) TLVs, which libqmi's
LOC RegisterEvents does not expose; and the engine never emits NMEA
indications (Get/SetNmeaTypes = NotSupported). So this client:
  1. looks up the LOC service (16) via the QRTR name service,
  2. InformClientRevision(2), RegisterEvents(position report | SV info,
     clientStrId "fp6", clientType default), Start(session 1,
     periodic fixes),
  3. writes GGA / RMC / GSV built from Position Report / GNSS SV Info
     indications to stdout (GeoClue [network-nmea] socket via socat),
  4. sends Stop(session 1) on SIGTERM/SIGINT/stdout EOF, so the engine
     powers down when GeoClue (or the user's location toggle) stops it.
It sends no other LOC request (no engine lock / NMEA types / assistance /
operation-mode change) and logs nothing position-related to the journal.
"""
import os
import select
import signal
import socket
import struct
import sys
import time

AF_QIPCRTR = 42
QRTR_PORT_CTRL = 0xFFFFFFFE
QRTR_TYPE_NEW_SERVER = 4
QRTR_TYPE_NEW_LOOKUP = 10
LOC_SERVICE = 16

MSG_INFORM_REV, MSG_REG_EVENTS, MSG_START, MSG_STOP = 0x20, 0x21, 0x22, 0x23
IND_POSITION, IND_SV_INFO = 0x24, 0x25
EVT_POSITION, EVT_SV_INFO = 0x01, 0x02
SESSION = 1

# eQMI_LOC_SV_SYSTEM_* -> NMEA talker
TALKER = {1: "GP", 2: "GA", 3: "GP", 4: "GB", 5: "GL", 6: "GB", 7: "GQ"}


def log(msg):
    print(f"fp6-loc-nmea: {msg}", file=sys.stderr, flush=True)


class Loc:
    def __init__(self):
        self.s = socket.socket(AF_QIPCRTR, socket.SOCK_DGRAM)
        self.txn = 0
        self.addr = self.lookup()

    def lookup(self, timeout=30):
        node = self.s.getsockname()[0]
        pkt = struct.pack("<IIIII", QRTR_TYPE_NEW_LOOKUP, LOC_SERVICE, 0, 0, 0)
        end = time.time() + timeout
        while time.time() < end:
            self.s.sendto(pkt, (node, QRTR_PORT_CTRL))
            t_end = time.time() + 2
            while time.time() < t_end:
                r, _, _ = select.select([self.s], [], [], 0.5)
                if not r:
                    continue
                d, _ = self.s.recvfrom(65536)
                if len(d) < 20:
                    continue
                cmd, svc, _inst, n, p = struct.unpack_from("<IIIII", d)
                if cmd == QRTR_TYPE_NEW_SERVER and svc == LOC_SERVICE:
                    return (n, p)
            time.sleep(1)
        raise SystemExit("fp6-loc-nmea: QMI LOC service not found on QRTR")

    @staticmethod
    def tlv(t, fmt, *v):
        b = struct.pack("<" + fmt, *v)
        return struct.pack("<BH", t, len(b)) + b

    def send(self, msg, tlvs=b""):
        self.txn = (self.txn + 1) & 0xFFFF or 1
        self.s.sendto(struct.pack("<BHHH", 0, self.txn, msg, len(tlvs)) + tlvs, self.addr)
        return self.txn

    @staticmethod
    def parse(d):
        if len(d) < 7:
            return None
        flags, txn, msg, ln = struct.unpack_from("<BHHH", d)
        tl, off = {}, 7
        while off + 3 <= min(len(d), 7 + ln):
            t, l = struct.unpack_from("<BH", d, off)
            tl[t] = d[off + 3: off + 3 + l]
            off += 3 + l
        return flags, txn, msg, tl

    def request(self, msg, tlvs=b"", timeout=5, on_ind=None):
        txn = self.send(msg, tlvs)
        end = time.time() + timeout
        while time.time() < end:
            r, _, _ = select.select([self.s], [], [], max(0, end - time.time()))
            if not r:
                break
            p = self.parse(self.s.recvfrom(65536)[0])
            if not p:
                continue
            flags, t, m, tl = p
            if flags == 2 and t == txn and m == msg:
                res, err = struct.unpack("<HH", tl.get(2, b"\0\0\0\0")[:4])
                return res, err
            if flags == 4 and on_ind:
                on_ind(m, tl)
        return None


def nmea(body):
    cs = 0
    for c in body.encode():
        cs ^= c
    return f"${body}*{cs:02X}\r\n"


def ddmm(v, lat):
    hemi = ("N" if v >= 0 else "S") if lat else ("E" if v >= 0 else "W")
    v = abs(v)
    deg = int(v)
    minutes = (v - deg) * 60
    return (f"{deg:02d}{minutes:07.4f}" if lat else f"{deg:03d}{minutes:07.4f}"), hemi


class Nmea:
    def __init__(self, out):
        self.out = out
        self.tracked = 0
        self.svs = []

    def write(self, s):
        self.out.write(s)
        self.out.flush()

    def sv_info(self, tl):
        b = tl.get(0x11) or tl.get(0x10)
        if not b:
            return
        n = b[0]
        if not n:
            self.svs, self.tracked = [], 0
            return
        ent = (len(b) - 1) // n
        if ent < 28:
            return
        svs = []
        for i in range(n):
            e = b[1 + i * ent: 1 + (i + 1) * ent]
            system, svid = struct.unpack_from("<IH", e, 4)
            status = struct.unpack_from("<I", e, 11)[0]
            el, az, snr = struct.unpack_from("<fff", e, 16)
            svs.append((system, svid, status, el, az, snr))
        self.svs = svs
        self.tracked = sum(1 for s in svs if s[2] == 3 or s[5] > 0)
        by = {}
        for s in svs:
            by.setdefault(TALKER.get(s[0], "GP"), []).append(s)
        for talker, lst in by.items():
            msgs = (len(lst) + 3) // 4
            for k in range(msgs):
                parts = [f"{talker}GSV", str(msgs), str(k + 1), str(len(lst))]
                for s in lst[k * 4:(k + 1) * 4]:
                    parts += [str(s[1]), str(int(max(0, s[3]))), str(int(s[4]) % 360),
                              str(int(s[5])) if s[5] > 0 else ""]
                self.write(nmea(",".join(parts)))

    def position(self, tl):
        status = struct.unpack("<I", tl[1][:4])[0] if 1 in tl else 1
        fix = status == 0 and 0x10 in tl and 0x11 in tl
        ts = struct.unpack("<Q", tl[0x25])[0] / 1000.0 if 0x25 in tl else time.time()
        t = time.gmtime(ts)
        hms = time.strftime("%H%M%S", t) + f".{int((ts % 1) * 100):02d}"
        dmy = time.strftime("%d%m%y", t)
        if not fix:
            self.write(nmea(f"GPGGA,{hms},,,,,0,{self.tracked:02d},,,,,,,"))
            self.write(nmea(f"GPRMC,{hms},V,,,,,,,{dmy},,,N"))
            return
        lat = struct.unpack("<d", tl[0x10])[0]
        lon = struct.unpack("<d", tl[0x11])[0]
        la, ns = ddmm(lat, True)
        lo, ew = ddmm(lon, False)
        alt = struct.unpack("<f", tl[0x1B])[0] if 0x1B in tl else None
        ell = struct.unpack("<f", tl[0x1A])[0] if 0x1A in tl else None
        sep = f"{ell - alt:.1f}" if alt is not None and ell is not None else ""
        hdop = f"{struct.unpack_from('<f', tl[0x24], 4)[0]:.1f}" if 0x24 in tl else ""
        if 0x2C in tl and tl[0x2C]:
            used = tl[0x2C][0]
        else:
            used = self.tracked
        spd = struct.unpack("<f", tl[0x18])[0] * 1.943844 if 0x18 in tl else 0.0
        hdg = f"{struct.unpack('<f', tl[0x20])[0]:.1f}" if 0x20 in tl else ""
        alts = f"{alt:.1f},M" if alt is not None else ","
        self.write(nmea(f"GPGGA,{hms},{la},{ns},{lo},{ew},1,{used:02d},{hdop},{alts},{sep},M,,"))
        self.write(nmea(f"GPRMC,{hms},A,{la},{ns},{lo},{ew},{spd:.1f},{hdg},{dmy},,,A"))


def main():
    # --no-stdin: manual/diagnostic runs (stdin EOF does not end the session)
    watch_stdin = "--no-stdin" not in sys.argv
    out = sys.stdout
    loc = Loc()
    gen = Nmea(out)
    stopping = []

    def on_ind(m, tl):
        try:
            if m == IND_POSITION:
                gen.position(tl)
            elif m == IND_SV_INFO:
                gen.sv_info(tl)
        except (BrokenPipeError, OSError):
            stopping.append(1)
        except (struct.error, KeyError, IndexError):
            pass

    def stop(*_):
        stopping.append(1)

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGPIPE, stop)

    r = loc.request(MSG_INFORM_REV, Loc.tlv(1, "I", 2))
    if r and r[0]:
        log(f"InformClientRevision rejected (error {r[1]}); continuing")
    sid = b"fp6\0"  # longer client strings are rejected (QMI error 19)
    r = loc.request(MSG_REG_EVENTS,
                    Loc.tlv(1, "Q", EVT_POSITION | EVT_SV_INFO)
                    + struct.pack("<BH", 0x10, len(sid)) + sid
                    + Loc.tlv(0x11, "I", 1))
    if not r or r[0]:
        raise SystemExit(f"fp6-loc-nmea: RegisterEvents failed: {r}")
    # session 1; 0x10 fix recurrence periodic (1); 0x12 intermediate
    # reports on (1); 0x13 minimum interval 1000 ms
    r = loc.request(MSG_START, Loc.tlv(1, "B", SESSION) + Loc.tlv(0x10, "I", 1)
                    + Loc.tlv(0x12, "I", 1) + Loc.tlv(0x13, "I", 1000))
    if not r or r[0]:
        raise SystemExit(f"fp6-loc-nmea: Start failed: {r}")
    log("LOC session started")
    try:
        while not stopping:
            rl, _, _ = select.select([loc.s, sys.stdin] if watch_stdin else [loc.s], [], [], 1)
            if watch_stdin and sys.stdin in rl:
                if not os.read(sys.stdin.fileno(), 4096):
                    # socat closes our stdin when GeoClue disconnects
                    break
            if loc.s in rl:
                p = Loc.parse(loc.s.recvfrom(65536)[0])
                if p and p[0] == 4:
                    on_ind(p[2], p[3])
    finally:
        loc.request(MSG_STOP, Loc.tlv(1, "B", SESSION), timeout=3)
        log("LOC session stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
