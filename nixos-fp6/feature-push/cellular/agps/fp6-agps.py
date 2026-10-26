"""fp6-agps: assisted GPS for the FP6 modem GNSS engine (raw QMI LOC over QRTR).

Python stdlib only. It speaks to the same modem LOC service as fp6-loc-nmea,
over its own QRTR client.

  probe          print what the modem asks for: the XTRA source (Get
                 Predicted Orbits Data Source), the XTRA validity and the
                 engine lock. No network access and no injection.
  xtra-download  fetch the XTRA (predicted orbits) file into the cache if it
                 is older than the refresh interval. It runs only when the
                 default route is an allowed network (wifi / mobile). The
                 request is a plain GET with a generic User-Agent and carries
                 no user data. The server sees the phone's IP address.
  inject         one-shot: XTRA (cached file), UTC time (only while the
                 kernel clock is NTP-synchronized) and coarse position (the
                 cached last fix and/or, if opted in, a BeaconDB lookup).
  serve          for as long as location is on (bound to fp6-gnss-nmea):
                 inject once, then answer the engine's own Inject Time /
                 Predicted Orbits / Position requests, and re-inject when a
                 new XTRA file lands in the cache.
  forget         delete the cached last fix.

Config: JSON file (--config, default /etc/fp6-agps.json). Nothing
position-related is logged; coordinates never reach the journal.
"""
import ctypes
import json
import os
import select
import signal
import socket
import ssl
import struct
import subprocess
import sys
import time
import urllib.error
import urllib.request

AF_QIPCRTR = 42
QRTR_PORT_CTRL = 0xFFFFFFFE
QRTR_TYPE_NEW_SERVER = 4
QRTR_TYPE_NEW_LOOKUP = 10
LOC_SERVICE = 16

MSG_INFORM_REV, MSG_REG_EVENTS = 0x20, 0x21
IND_INJECT_TIME_REQ, IND_INJECT_ORBITS_REQ, IND_INJECT_POS_REQ = 0x28, 0x29, 0x2A
IND_ENGINE_STATE = 0x2B
MSG_INJECT_ORBITS, MSG_GET_ORBITS_SOURCE, MSG_GET_ORBITS_VALIDITY = 0x35, 0x36, 0x37
MSG_INJECT_UTC, MSG_INJECT_POSITION = 0x38, 0x39
MSG_GET_ENGINE_LOCK = 0x3B
MSG_INJECT_XTRA = 0xA7
EVT_INJECT_TIME_REQ, EVT_INJECT_ORBITS_REQ, EVT_INJECT_POS_REQ = 0x10, 0x20, 0x40
EVT_ENGINE_STATE = 0x80

QMI_ERR = {1: "MalformedMessage", 3: "Internal", 19: "ArgTooLong", 48: "InvalidArgument",
           71: "InvalidQmiCommand", 94: "NotSupported"}
IND_STATUS = {0: "success", 1: "general failure", 2: "unsupported", 3: "invalid parameter",
              4: "engine busy", 5: "phone offline", 6: "timeout"}
LOCK = {1: "none", 2: "mi", 3: "mt", 4: "all"}

DEFAULTS = {
    "cacheDir": "/var/cache/fp6-agps",
    "stateDir": "/var/lib/fp6-agps",
    "gnssUnit": "fp6-gnss-nmea.service",
    "xtra": {
        "enable": True,
        "download": True,
        # used when the modem does not name its own servers
        # what this modem (MPSS.DE.3.1.4) names itself; xtra3 "M" format,
        # GPS + GLONASS + BeiDou + Galileo + QZSS
        "servers": ["https://path1.xtracloud.net/xtra3Mgrbej.bin",
                    "https://path2.xtracloud.net/xtra3Mgrbej.bin",
                    "https://path3.xtracloud.net/xtra3Mgrbej.bin"],
        "preferModemServers": True,
        "networks": ["wifi", "mobile"],
        "refreshHours": 24,
        "maxAgeDays": 7,
        "downloadWhenLocationOff": True,
        "userAgent": "fp6-agps/1",
    },
    "time": {"enable": True, "maxUncertaintyMs": 10000},
    "position": {
        "lastFix": True,
        "lastFixMaxAgeDays": 30,
        "network": False,
        "networkUrl": "https://api.beacondb.net/v1/geolocate",
        "userAgent": "fp6-agps/1",
    },
}


def log(msg):
    print(f"fp6-agps: {msg}", file=sys.stderr, flush=True)


def merge(base, over):
    out = dict(base)
    for k, v in (over or {}).items():
        out[k] = merge(base[k], v) if isinstance(v, dict) and isinstance(base.get(k), dict) else v
    return out


def load_config(path):
    try:
        with open(path) as f:
            return merge(DEFAULTS, json.load(f))
    except FileNotFoundError:
        return merge(DEFAULTS, {})


# ---- QMI LOC over QRTR ---------------------------------------------------

def tlv(t, fmt, *v):
    b = struct.pack("<" + fmt, *v)
    return struct.pack("<BH", t, len(b)) + b


def tlv_raw(t, b):
    return struct.pack("<BH", t, len(b)) + b


class Loc:
    def __init__(self, timeout=30):
        self.s = socket.socket(AF_QIPCRTR, socket.SOCK_DGRAM)
        self.txn = 0
        self.pending = []      # indications seen while waiting for something else
        self.addr = self.lookup(timeout)

    def lookup(self, timeout):
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
                if len(d) >= 20:
                    cmd, svc, _i, n, p = struct.unpack_from("<IIIII", d)
                    if cmd == QRTR_TYPE_NEW_SERVER and svc == LOC_SERVICE:
                        return (n, p)
            time.sleep(1)
        raise SystemExit("fp6-agps: QMI LOC service not found on QRTR")

    @staticmethod
    def parse(d):
        if len(d) < 7:
            return None
        flags, txn, msg, ln = struct.unpack_from("<BHHH", d)
        tl, off = {}, 7
        while off + 3 <= min(len(d), 7 + ln):
            t, n = struct.unpack_from("<BH", d, off)
            tl[t] = d[off + 3: off + 3 + n]
            off += 3 + n
        return flags, txn, msg, tl

    def recv(self, timeout):
        r, _, _ = select.select([self.s], [], [], max(0, timeout))
        if not r:
            return None
        return self.parse(self.s.recvfrom(65536)[0])

    def request(self, msg, tlvs=b"", timeout=5):
        """Send a request; return (qmi_result, qmi_error) or None on timeout."""
        self.txn = (self.txn + 1) & 0xFFFF or 1
        txn = self.txn
        self.s.sendto(struct.pack("<BHHH", 0, txn, msg, len(tlvs)) + tlvs, self.addr)
        end = time.time() + timeout
        while time.time() < end:
            p = self.recv(end - time.time())
            if not p:
                break
            flags, t, m, tl = p
            if flags == 2 and t == txn and m == msg:
                return struct.unpack("<HH", tl.get(2, b"\0\0\0\0")[:4])
            if flags == 4:
                self.pending.append((m, tl))
        return None

    def wait_ind(self, msg, timeout=10):
        """Wait for an indication with id msg (an int or a tuple of ids);
        returns its TLVs (or (id, TLVs) when msg is a tuple)."""
        ids = msg if isinstance(msg, tuple) else (msg,)
        found = None
        for i, (m, tl) in enumerate(self.pending):
            if m in ids:
                del self.pending[i]
                found = (m, tl)
                break
        end = time.time() + timeout
        while found is None and time.time() < end:
            p = self.recv(end - time.time())
            if not p:
                break
            if p[0] == 4:
                if p[2] in ids:
                    found = (p[2], p[3])
                else:
                    self.pending.append((p[2], p[3]))
        if found is None:
            return None
        return found if isinstance(msg, tuple) else found[1]

    def call(self, msg, tlvs=b"", timeout=10):
        """Request whose real answer is an indication of the same id.
        Returns (error_string_or_None, indication_tlvs)."""
        r = self.request(msg, tlvs)
        if r is None:
            return "no response", {}
        if r[0]:
            return f"QMI error {QMI_ERR.get(r[1], r[1])}", {}
        ind = self.wait_ind(msg, timeout)
        if ind is None:
            return "no indication", {}
        st = struct.unpack("<I", ind[1][:4])[0] if 1 in ind else 0
        return (None if st == 0 else f"status {IND_STATUS.get(st, st)}"), ind

    def register(self, mask):
        r = self.request(MSG_INFORM_REV, tlv(1, "I", 2))
        sid = b"fp6\0"   # longer client strings are rejected (QMI error 19)
        r = self.request(MSG_REG_EVENTS, tlv(1, "Q", mask) + tlv_raw(0x10, sid) + tlv(0x11, "I", 1))
        if not r or r[0]:
            raise SystemExit(f"fp6-agps: RegisterEvents failed: {r}")


def parse_strings(b):
    """QMI array (u8 count) of strings (u8 length prefix)."""
    out, off = [], 1
    for _ in range(b[0] if b else 0):
        n = b[off]
        out.append(b[off + 1: off + 1 + n].decode(errors="replace").rstrip("\0"))
        off += 1 + n
    return out


def orbits_info(ind, sizes_tlv, servers_tlv):
    info = {}
    if sizes_tlv in ind and len(ind[sizes_tlv]) >= 8:
        info["maxFileSize"], info["maxPartSize"] = struct.unpack_from("<II", ind[sizes_tlv])
    if servers_tlv in ind:
        try:
            info["servers"] = parse_strings(ind[servers_tlv])
        except IndexError:
            info["servers"] = []
    return info


def orbits_source(loc):
    """Get Predicted Orbits Data Source. This firmware answers with an
    "Inject Predicted Orbits Request" indication (0x29: sizes 0x01, server
    list 0x10, file info 0x14) instead of the 0x36 indication."""
    r = loc.request(MSG_GET_ORBITS_SOURCE)
    if r is None:
        return {"error": "no response"}
    if r[0]:
        return {"error": f"QMI error {QMI_ERR.get(r[1], r[1])}"}
    got = loc.wait_ind((MSG_GET_ORBITS_SOURCE, IND_INJECT_ORBITS_REQ))
    if got is None:
        return {"error": "no indication"}
    m, ind = got
    if m == IND_INJECT_ORBITS_REQ:
        info = orbits_info(ind, 0x01, 0x10)
        if 0x14 in ind and len(ind[0x14]) >= 8:
            info["fileType"], info["intervalMinutes"] = struct.unpack_from("<II", ind[0x14])
    else:
        info = orbits_info(ind, 0x10, 0x11)
    info["error"] = None
    return info


def orbits_validity(loc):
    err, ind = loc.call(MSG_GET_ORBITS_VALIDITY)
    info = {"error": err}
    if 0x10 in ind:
        start_ms, hours = struct.unpack_from("<QH", ind[0x10])
        info["startUtc"] = start_ms / 1000.0
        info["durationHours"] = hours
    return info


# ---- XTRA ----------------------------------------------------------------

def default_route_kind():
    """'wifi', 'mobile', 'other' or None (no default route)."""
    try:
        with open("/proc/net/route") as f:
            rows = [ln.split() for ln in f.readlines()[1:]]
    except OSError:
        return None
    best = None
    for r in rows:
        if len(r) > 7 and r[1] == "00000000" and r[7] == "00000000" and int(r[3], 16) & 1:
            metric = int(r[6])
            if best is None or metric < best[0]:
                best = (metric, r[0])
    if not best:
        return None
    dev = best[1]
    if os.path.isdir(f"/sys/class/net/{dev}/wireless") or dev.startswith("wl"):
        return "wifi"
    if dev.startswith(("rmnet", "wwan", "ccmni")):
        return "mobile"
    return "other"


def xtra_paths(cfg):
    d = cfg["cacheDir"]
    return os.path.join(d, "xtra.bin"), os.path.join(d, "xtra.json")


def read_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def write_atomic(path, data):
    tmp = path + ".tmp"
    with open(tmp, "wb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def ssl_context():
    ctx = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE") or None)
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    return ctx


def xtra_servers(cfg, meta):
    servers = []
    if cfg["xtra"]["preferModemServers"]:
        servers += [u for u in meta.get("modemServers", []) if u.startswith(("https://", "http://"))]
    servers += cfg["xtra"]["servers"]
    seen, out = set(), []
    for u in servers:
        if u not in seen:
            seen.add(u)
            out.append(u)
    return out


def cmd_xtra_download(cfg, force=False):
    xc = cfg["xtra"]
    if not (xc["enable"] and xc["download"]):
        log("XTRA download disabled")
        return 0
    if not xc["downloadWhenLocationOff"] and not unit_active(cfg["gnssUnit"]):
        log("XTRA download skipped: location is off (downloadWhenLocationOff = false)")
        return 0
    kind = default_route_kind()
    if kind not in xc["networks"]:
        log(f"XTRA download skipped: default route is {kind or 'none'} (allowed: {', '.join(xc['networks'])})")
        return 0
    path, mpath = xtra_paths(cfg)
    meta = read_json(mpath)
    age = time.time() - meta.get("fetched", 0)
    if not force and os.path.exists(path) and age < xc["refreshHours"] * 3600:
        log(f"XTRA cache fresh ({age / 3600:.1f} h old)")
        return 0
    maxsize = meta.get("maxFileSize") or 1 << 20
    ctx = ssl_context()
    for url in xtra_servers(cfg, meta):
        req = urllib.request.Request(url, headers={"User-Agent": xc["userAgent"], "Accept": "*/*"})
        if not force and meta.get("url") == url and os.path.exists(path) and meta.get("lastModified"):
            req.add_header("If-Modified-Since", meta["lastModified"])
        try:
            with urllib.request.urlopen(req, timeout=30, context=ctx) as r:
                data = r.read(maxsize + 1)
                lastmod = r.headers.get("Last-Modified", "")
        except urllib.error.HTTPError as e:
            if e.code == 304:
                meta["fetched"] = time.time()
                write_atomic(mpath, json.dumps(meta).encode())
                log(f"XTRA unchanged on {url}")
                return 0
            log(f"XTRA {url}: HTTP {e.code}")
            continue
        except (OSError, ssl.SSLError) as e:
            log(f"XTRA {url}: {e}")
            continue
        if len(data) < 1024 or len(data) > maxsize:
            log(f"XTRA {url}: implausible size {len(data)}")
            continue
        write_atomic(path, data)
        meta.update({"url": url, "fetched": time.time(), "lastModified": lastmod,
                     "size": len(data), "via": kind})
        write_atomic(mpath, json.dumps(meta).encode())
        log(f"XTRA downloaded from {url} ({len(data)} bytes, via {kind})")
        return 0
    log("XTRA download failed on every server")
    return 1


def inject_xtra(loc, cfg):
    path, mpath = xtra_paths(cfg)
    meta = read_json(mpath)
    try:
        st = os.stat(path)
    except OSError:
        log("XTRA: no cached file yet")
        return False
    if time.time() - meta.get("fetched", st.st_mtime) > cfg["xtra"]["maxAgeDays"] * 86400:
        log("XTRA: cached file too old, not injected")
        return False
    with open(path, "rb") as f:
        data = f.read()
    src = orbits_source(loc)
    part = min(1024, src.get("maxPartSize") or 1024)
    if src.get("maxFileSize") and len(data) > src["maxFileSize"]:
        log(f"XTRA: file ({len(data)} B) larger than the modem accepts ({src['maxFileSize']} B)")
        return False
    parts = (len(data) + part - 1) // part
    t0 = time.time()
    # this firmware: Inject Xtra Data (0xA7) works, Inject Predicted Orbits
    # Data (0x35) is NotSupported; keep 0x35 as the fallback for others
    for msg in (MSG_INJECT_XTRA, MSG_INJECT_ORBITS):
        ok = True
        for i in range(parts):
            chunk = data[i * part:(i + 1) * part]
            body = (tlv(1, "I", len(data)) + tlv(2, "H", parts) + tlv(3, "H", i + 1)
                    + tlv_raw(4, struct.pack("<H", len(chunk)) + chunk))
            if msg == MSG_INJECT_ORBITS:
                body += tlv(0x10, "I", 0)      # format: XTRA
            err, _ind = loc.call(msg, body, timeout=15)
            if err:
                ok = False
                log(f"XTRA: {'Inject Predicted Orbits Data' if msg == MSG_INJECT_ORBITS else 'Inject Xtra Data'}"
                    f" part {i + 1}/{parts}: {err}")
                break
        if ok:
            log(f"XTRA injected ({len(data)} B in {parts} parts, {time.time() - t0:.1f} s)")
            return True
        if not err.startswith("QMI error"):
            return False
    return False


# ---- time ----------------------------------------------------------------

def ntp_state():
    """(synchronized, maxerror_ms) from adjtimex(2), read-only."""
    buf = ctypes.create_string_buffer(512)
    libc = ctypes.CDLL(None, use_errno=True)
    state = libc.adjtimex(buf)
    maxerror_us = struct.unpack_from("<q", buf, 24)[0]
    status = struct.unpack_from("<i", buf, 40)[0]
    synced = state != 5 and not status & 0x40   # TIME_ERROR / STA_UNSYNC
    return synced, maxerror_us / 1000.0


def inject_time(loc, cfg):
    if not cfg["time"]["enable"]:
        return False
    synced, maxerr = ntp_state()
    if not synced:
        log("time: system clock not NTP-synchronized, not injected")
        return False
    unc = int(min(max(maxerr, 20), cfg["time"]["maxUncertaintyMs"]))
    now = int(time.time() * 1000)
    err, _ = loc.call(MSG_INJECT_UTC, tlv(1, "Q", now) + tlv(2, "I", unc) + tlv(0x10, "i", 2))
    if err:
        log(f"time: Inject UTC Time: {err}")
        return False
    log(f"time injected (uncertainty {unc} ms)")
    return True


# ---- position ------------------------------------------------------------

def last_fix(cfg):
    d = read_json(os.path.join(cfg["stateDir"], "last-fix.json"))
    if not d or "lat" not in d:
        return None
    age = time.time() - d.get("t", 0)
    if age > cfg["position"]["lastFixMaxAgeDays"] * 86400:
        return None
    # stored rounded (grid in d["grid"] degrees); people move: grow the
    # uncertainty with the age of the fix (60 km/h for the first hour, then
    # capped at 200 km, which still helps satellite visibility a lot)
    unc = 6000.0 + min(200000.0, age * 16.7)
    return d["lat"], d["lon"], unc, int(d.get("t", 0) * 1000), 0


def wifi_scan():
    try:
        out = subprocess.run(["nmcli", "-t", "-e", "yes", "-f", "SSID,BSSID,SIGNAL,FREQ",
                              "device", "wifi", "list", "--rescan", "auto"],
                             capture_output=True, text=True, timeout=20).stdout
    except (OSError, subprocess.SubprocessError):
        return []
    aps = []
    for ln in out.splitlines():
        # SSID may contain escaped ':' ("\:"); BSSID is escaped too
        fields, cur, esc = [], "", False
        for c in ln:
            if esc:
                cur += c
                esc = False
            elif c == "\\":
                esc = True
            elif c == ":" and len(fields) < 3:
                fields.append(cur)
                cur = ""
            else:
                cur += c
        fields.append(cur)
        if len(fields) != 4:
            continue
        ssid, bssid, sig, freq = fields
        # honour the "_nomap" opt-out; skip hidden networks
        if not ssid or ssid.endswith("_nomap"):
            continue
        try:
            sig = int(sig)
            mhz = int(freq.split()[0])
        except ValueError:
            continue
        aps.append({"macAddress": bssid.lower(), "signalStrength": sig // 2 - 100, "frequency": mhz})
    return aps


def network_fix(cfg):
    pc = cfg["position"]
    aps = wifi_scan()
    if len(aps) < 2:
        log(f"network position: {len(aps)} usable WiFi access points, not queried")
        return None
    body = json.dumps({"considerIp": False, "wifiAccessPoints": aps}).encode()
    req = urllib.request.Request(pc["networkUrl"], data=body, method="POST",
                                 headers={"Content-Type": "application/json", "User-Agent": pc["userAgent"]})
    try:
        with urllib.request.urlopen(req, timeout=20, context=ssl_context()) as r:
            ans = json.load(r)
    except (OSError, ValueError, ssl.SSLError) as e:
        log(f"network position: lookup failed ({e})")
        return None
    try:
        lat, lon = float(ans["location"]["lat"]), float(ans["location"]["lng"])
        acc = float(ans.get("accuracy", 5000))
    except (KeyError, TypeError, ValueError):
        log("network position: no result")
        return None
    log(f"network position: {len(aps)} APs sent, accuracy {acc:.0f} m")
    return lat, lon, max(acc, 100.0), int(time.time() * 1000), 3


def inject_position(loc, cfg, network=True):
    pc = cfg["position"]
    fix = None
    if network and pc["network"]:
        fix = network_fix(cfg)
    if fix is None and pc["lastFix"]:
        fix = last_fix(cfg)
        if fix is None:
            log("position: no recent cached fix")
    if fix is None:
        return False
    lat, lon, unc, ts, src = fix
    body = (tlv(0x10, "d", lat) + tlv(0x11, "d", lon) + tlv(0x12, "f", unc)
            + tlv(0x13, "B", 68) + tlv(0x14, "I", 2 if src == 0 else 3)
            + tlv(0x1B, "Q", ts) + tlv(0x1D, "I", 6 if src == 0 else src))
    err, _ = loc.call(MSG_INJECT_POSITION, body)
    if err:
        log(f"position: Inject Position: {err}")
        return False
    log(f"coarse position injected ({'network' if src else 'cached fix'}, uncertainty {unc / 1000:.0f} km)")
    return True


# ---- commands ------------------------------------------------------------

def cmd_probe(cfg):
    loc = Loc()
    loc.register(EVT_INJECT_ORBITS_REQ)   # the orbits-source answer arrives as that event
    r = loc.request(MSG_GET_ENGINE_LOCK)
    ind = loc.wait_ind(MSG_GET_ENGINE_LOCK) if r and not r[0] else None
    lock = struct.unpack("<I", ind[0x10][:4])[0] if ind and 0x10 in ind else None
    print(f"engine lock: {LOCK.get(lock, lock)}")
    src = orbits_source(loc)
    print(f"predicted orbits source: {json.dumps(src)}")
    val = orbits_validity(loc)
    if "startUtc" in val:
        val["start"] = time.strftime("%Y-%m-%d %H:%M UTC", time.gmtime(val["startUtc"]))
    print(f"predicted orbits validity: {json.dumps(val)}")
    synced, maxerr = ntp_state()
    print(f"system clock NTP-synchronized: {synced} (maxerror {maxerr:.0f} ms)")
    print(f"default route: {default_route_kind()}")
    remember_modem_source(cfg, src)
    return 0


def remember_modem_source(cfg, src):
    """Keep the modem's server list / size limits for xtra-download (which
    must not need the modem). Best effort."""
    _path, mpath = xtra_paths(cfg)
    if not os.access(cfg["cacheDir"], os.W_OK):
        return
    meta = read_json(mpath)
    changed = False
    if src.get("servers") is not None and meta.get("modemServers") != src["servers"]:
        meta["modemServers"] = src["servers"]
        changed = True
    if src.get("maxFileSize") and meta.get("maxFileSize") != src["maxFileSize"]:
        meta["maxFileSize"] = src["maxFileSize"]
        changed = True
    if changed:
        write_atomic(mpath, json.dumps(meta).encode())


def unit_active(unit):
    r = subprocess.run(["systemctl", "is-active", "--quiet", unit])
    return r.returncode == 0


def cmd_inject(cfg, what, require_location=True):
    if require_location and not unit_active(cfg["gnssUnit"]):
        log("location is off: nothing injected")
        return 0
    loc = Loc()
    loc.register(EVT_INJECT_ORBITS_REQ)   # the orbits-source answer arrives as that event
    remember_modem_source(cfg, orbits_source(loc))
    if "time" in what:
        inject_time(loc, cfg)
    if "xtra" in what and cfg["xtra"]["enable"]:
        inject_xtra(loc, cfg)
    if "position" in what:
        inject_position(loc, cfg)
    return 0


class Stop(Exception):
    pass


def on_stop_signal(*_):
    # raise out of select() at once: the toggle-off (StopUnit) waits for us
    raise Stop()


def cmd_serve(cfg):
    signal.signal(signal.SIGTERM, on_stop_signal)
    signal.signal(signal.SIGINT, on_stop_signal)
    try:
        serve(cfg)
    except Stop:
        pass
    log("stopped")
    return 0


def serve(cfg):
    loc = Loc()
    loc.register(EVT_INJECT_TIME_REQ | EVT_INJECT_ORBITS_REQ | EVT_INJECT_POS_REQ | EVT_ENGINE_STATE)
    src = orbits_source(loc)
    remember_modem_source(cfg, src)
    inject_time(loc, cfg)
    xtra_path, _ = xtra_paths(cfg)
    xtra_mtime = None
    if cfg["xtra"]["enable"] and inject_xtra(loc, cfg):
        xtra_mtime = os.stat(xtra_path).st_mtime
    inject_position(loc, cfg)
    last = {"time": time.time(), "pos": time.time(), "xtra": time.time()}
    log("serving engine assistance requests")
    while True:
        while loc.pending:
            m, tl = loc.pending.pop(0)
            handle_ind(loc, cfg, m, tl, last)
        p = loc.recv(30)
        if p and p[0] == 4:
            handle_ind(loc, cfg, p[2], p[3], last)
        # a fresh XTRA download (timer) is injected while location is on;
        # a file that fails is not retried until it changes again
        try:
            mt = os.stat(xtra_path).st_mtime
        except OSError:
            mt = None
        if cfg["xtra"]["enable"] and mt and mt != xtra_mtime:
            xtra_mtime = mt
            inject_xtra(loc, cfg)


def handle_ind(loc, cfg, m, tl, last):
    now = time.time()
    if m == IND_INJECT_TIME_REQ:
        log("engine requested time")
        if now - last["time"] > 5:
            inject_time(loc, cfg)
            last["time"] = now
    elif m == IND_INJECT_ORBITS_REQ:
        remember_modem_source(cfg, orbits_info(tl, 0x01, 0x10))
        # every Get Predicted Orbits Data Source is answered with this
        # indication too: only re-inject when the last one is > 10 min old
        if cfg["xtra"]["enable"] and now - last["xtra"] > 600:
            log("engine requested predicted orbits")
            last["xtra"] = now
            inject_xtra(loc, cfg)
    elif m == IND_INJECT_POS_REQ:
        log("engine requested a position")
        if now - last["pos"] > 60:
            inject_position(loc, cfg)
            last["pos"] = now
    elif m == IND_ENGINE_STATE and 1 in tl:
        st = struct.unpack("<i", tl[1][:4])[0]
        log(f"engine {'on' if st == 1 else 'off'}")


def cmd_forget(cfg):
    try:
        os.unlink(os.path.join(cfg["stateDir"], "last-fix.json"))
        log("cached last fix deleted")
    except FileNotFoundError:
        log("no cached last fix")
    return 0


def main(argv):
    cfgpath = "/etc/fp6-agps.json"
    args = []
    it = iter(argv[1:])
    for a in it:
        if a == "--config":
            cfgpath = next(it)
        else:
            args.append(a)
    cfg = load_config(cfgpath)
    cmd = args[0] if args else "probe"
    if cmd == "probe":
        return cmd_probe(cfg)
    if cmd == "xtra-download":
        return cmd_xtra_download(cfg, force="--force" in args)
    if cmd == "inject":
        what = [a for a in args[1:] if a in ("time", "xtra", "position")] or ["time", "xtra", "position"]
        return cmd_inject(cfg, what, require_location="--ignore-location-setting" not in args)
    if cmd == "serve":
        return cmd_serve(cfg)
    if cmd == "forget":
        return cmd_forget(cfg)
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
