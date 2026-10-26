"""fp6-ims-pdn: hold the carrier IMS PDN for imsd next to oFono (raw QMI WDS over QRTR).

Why not oFono's own "ims" context: oFono's qmimodem driver starts every
context by APN string. With the LTE attach PDN granted IPv4 only, this modem
refuses an APN-string IPv6 call (imsd's F6 finding, also seen on the FP6 with
Orange RO). A call started by 3GPP profile index is exempt, and the carrier
MCFG already provisions an "ims" profile. oFono has no profile-index path, so
this helper does what ModemManager's bearer did for imsd on postmarketOS/Nura:

  1. QRTR lookup of WDS (service 1).
  2. One WDS client: Bind Mux Data Port (embedded/1, mux MUX, the same binding
     oFono uses for its premux links), Set IP Family 6, Start Network (3GPP
     profile index, else APN string), Get Current Settings (IPv6 address,
     prefix, MTU, P-CSCF list from the PCO).
  3. Configures the rmnet netdev (no default route, MTU, address, DAD wait),
     a host route to the first P-CSCF, and writes /run/imsd.env
     (DEV, LOCAL, PCSCF_DISCOVERED, PCSCF_DISCOVERY) for imsd.
  4. Tells systemd READY=1 and keeps the client open: the call lives as long
     as this client. A Packet Service Status "disconnected" indication, or a
     vanished netdev, ends the process non-zero (systemd restarts it).
  5. SIGTERM: Stop Network, exit 0.

Runtime only: no profile, NV, EFS or PDC write is ever sent. It never prints
the IMSI, ICCID, IMEI or MSISDN; addresses go only to /run/imsd.env (0600).
"""
import argparse
import ipaddress
import os
import select
import signal
import socket
import struct
import subprocess
import sys
import time

AF_QIPCRTR = 42
QRTR_PORT_CTRL = 0xFFFFFFFE
QRTR_TYPE_NEW_SERVER = 4
QRTR_TYPE_NEW_LOOKUP = 10
WDS = 1

START_NETWORK, STOP_NETWORK, PKT_STATUS = 0x0020, 0x0021, 0x0022
GET_CURRENT_SETTINGS, SET_IP_FAMILY, BIND_MUX = 0x002D, 0x004D, 0x00A2
QMI_ERR = {0: "None", 1: "MalformedMessage", 3: "Internal", 10: "InvalidProfile",
           14: "CallFailed", 15: "OutOfCall", 17: "MissingArgument", 26: "NoEffect",
           48: "InvalidArgument", 70: "InvalidOperation", 82: "AccessDenied"}
# QMI WDS verbose call end reason types (libqmi QmiWdsVerboseCallEndReasonType)
VCER_TYPE = {0: "unspecified", 1: "mobile-ip", 2: "internal", 3: "cm", 6: "3gpp",
             7: "ppp", 8: "ehrpd", 9: "ipv6"}


def log(msg):
    print(f"fp6-ims-pdn: {msg}", file=sys.stderr, flush=True)


def tlv(t, payload):
    return struct.pack("<BH", t, len(payload)) + payload


class Wds:
    def __init__(self, timeout=30):
        self.s = socket.socket(AF_QIPCRTR, socket.SOCK_DGRAM)
        self.txn = 0
        self.addr = self._lookup(timeout)
        self.pending = []  # indications received while waiting for a response

    def _lookup(self, timeout):
        node = self.s.getsockname()[0]
        pkt = struct.pack("<IIIII", QRTR_TYPE_NEW_LOOKUP, WDS, 0, 0, 0)
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
                if cmd == QRTR_TYPE_NEW_SERVER and svc == WDS and (n, p) != (0, 0):
                    return (n, p)
            time.sleep(1)
        raise SystemExit("fp6-ims-pdn: QMI WDS service not found on QRTR")

    @staticmethod
    def parse(d):
        if len(d) < 7:
            return None
        typ, txn, msg, ln = struct.unpack_from("<BHHH", d)
        tl, off = {}, 7
        while off + 3 <= 7 + ln and off + 3 <= len(d):
            t, n = struct.unpack_from("<BH", d, off)
            tl[t] = d[off + 3:off + 3 + n]
            off += 3 + n
        return typ, txn, msg, tl

    def request(self, msg, tlvs=b"", timeout=60):
        self.txn = (self.txn + 1) & 0xFFFF or 1
        txn = self.txn
        self.s.sendto(struct.pack("<BHHH", 0, txn, msg, len(tlvs)) + tlvs, self.addr)
        end = time.time() + timeout
        while time.time() < end:
            r, _, _ = select.select([self.s], [], [], 0.5)
            if not r:
                continue
            d, src = self.s.recvfrom(65536)
            p = self.parse(d)
            if not p or src != self.addr:
                continue
            typ, rtxn, rmsg, tl = p
            if typ == 4:
                self.pending.append((rmsg, tl))
                continue
            if typ == 2 and rtxn == txn and rmsg == msg:
                res, err = struct.unpack_from("<HH", tl.get(2, b"\0\0\0\0"))
                return err if res else 0, tl
        return -1, {}

    def indication(self, timeout):
        if self.pending:
            return self.pending.pop(0)
        r, _, _ = select.select([self.s], [], [], timeout)
        if not r:
            return None
        d, src = self.s.recvfrom(65536)
        p = self.parse(d)
        if p and src == self.addr and p[0] == 4:
            return p[2], p[3]
        return None


def end_reason(tl):
    out = []
    if 0x10 in tl and len(tl[0x10]) >= 2:
        out.append(f"call-end-reason {struct.unpack_from('<H', tl[0x10])[0]}")
    if 0x11 in tl and len(tl[0x11]) >= 4:
        t, r = struct.unpack_from("<Hh", tl[0x11])
        out.append(f"verbose {VCER_TYPE.get(t, t)}/{r}")
    return ", ".join(out) or "no reason given"


def ip6(b):
    return str(ipaddress.IPv6Address(bytes(b[:16])))


def settings(tl):
    s = {}
    if 0x25 in tl and len(tl[0x25]) >= 17:
        s["addr"], s["prefix"] = ip6(tl[0x25]), tl[0x25][16]
    if 0x29 in tl and len(tl[0x29]) >= 4:
        s["mtu"] = struct.unpack_from("<I", tl[0x29])[0]
    if 0x22 in tl and tl[0x22]:
        s["pco"] = tl[0x22][0]
    pcscf = []
    v = tl.get(0x2E, b"")
    if v and len(v) == 1 + 16 * v[0]:
        pcscf = [ip6(v[1 + 16 * i:17 + 16 * i]) for i in range(v[0])]
    s["pcscf"] = pcscf
    v4 = tl.get(0x23, b"")
    s["pcscf_v4"] = v4[0] if v4 and len(v4) == 1 + 4 * v4[0] else 0
    if 0x1E in tl:
        s["has_v4"] = True
    return s


def sh(*cmd, check=True):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if check and r.returncode:
        raise RuntimeError(f"{' '.join(cmd)}: {r.stderr.strip()}")
    return r


def sd_notify(state):
    path = os.environ.get("NOTIFY_SOCKET")
    if not path:
        return
    if path.startswith("@"):
        path = "\0" + path[1:]
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as u:
            u.sendto(state.encode(), path)
    except OSError:
        pass


def ensure_link(dev, mux):
    if not os.path.exists(f"/sys/class/net/{dev}"):
        sh("ip", "link", "add", "link", "rmnet_ipa0", "name", dev, "type", "rmnet",
           "mux_id", str(mux), "ingress-deaggregation", "on",
           "ingress-mapv5-checksum", "on", "egress-mapv5-checksum", "on")
        log(f"created {dev} (mux {mux})")


def configure(dev, st, envfile):
    for k, v in (("accept_ra_defrtr", "0"), ("autoconf", "0")):
        try:
            with open(f"/proc/sys/net/ipv6/conf/{dev}/{k}", "w") as f:
                f.write(v)
        except OSError:
            pass
    if st.get("mtu", 0) >= 1280:
        sh("ip", "link", "set", dev, "mtu", str(st["mtu"]), check=False)
    sh("ip", "link", "set", dev, "up")
    sh("ip", "-6", "addr", "flush", "dev", dev, "scope", "global", check=False)
    sh("ip", "-6", "addr", "replace", f"{st['addr']}/{st.get('prefix') or 64}", "dev", dev, "nodad")
    sh("ip", "-6", "route", "flush", "default", "dev", dev, check=False)
    if st["pcscf"]:
        sh("ip", "-6", "route", "replace", f"{st['pcscf'][0]}/128", "dev", dev)
    disc = "found" if st["pcscf"] else "none"
    tmp = envfile + ".tmp"
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(f"DEV={dev}\nLOCAL={st['addr']}\nPCSCF_DISCOVERED={','.join(st['pcscf'][:8])}\n"
                f"PCSCF_DISCOVERY={disc}\n")
    os.replace(tmp, envfile)
    log(f"{dev} up (prefix /{st.get('prefix')}, mtu {st.get('mtu')}), "
        f"{len(st['pcscf'])} IPv6 P-CSCF(s) from the PCO; wrote {envfile}")


def start(w, args):
    """Start the IMS call; returns (packet handle, settings) or exits."""
    ep = struct.pack("<II", 4, 1)  # embedded, interface 1 (IPA)
    err, _ = w.request(BIND_MUX, tlv(0x10, ep) + tlv(0x11, bytes([args.mux])) +
                       tlv(0x13, struct.pack("<I", 1)))
    if err:
        raise SystemExit(f"fp6-ims-pdn: BindMuxDataPort(mux {args.mux}) failed: {QMI_ERR.get(err, err)}")
    err, _ = w.request(SET_IP_FAMILY, tlv(0x01, bytes([args.family])))
    if err and err != 26:
        log(f"SetIpFamily({args.family}): {QMI_ERR.get(err, err)} (continuing)")
    attempts = []
    if args.profile:
        attempts.append(("profile", tlv(0x31, bytes([args.profile]))))
    if args.apn:
        attempts.append(("apn", tlv(0x14, args.apn.encode())))
    for kind, sel in attempts:
        t = sel + tlv(0x19, bytes([args.family]))
        err, tl = w.request(START_NETWORK, t, timeout=args.start_timeout)
        if err == 0 and 0x01 in tl:
            handle = struct.unpack_from("<I", tl[0x01])[0]
            log(f"IMS call up by {kind} (family {args.family}, mux {args.mux})")
            break
        log(f"StartNetwork by {kind} failed: {QMI_ERR.get(err, err)} ({end_reason(tl)})")
    else:
        raise SystemExit(2)
    err, tl = w.request(GET_CURRENT_SETTINGS, tlv(0x10, struct.pack("<I", 0x7FFFF)))
    if err:
        log(f"GetCurrentSettings: {QMI_ERR.get(err, err)}")
        stop(w, handle)
        raise SystemExit(1)
    return handle, settings(tl)


def stop(w, handle):
    err, _ = w.request(STOP_NETWORK, tlv(0x01, struct.pack("<I", handle)), timeout=20)
    log(f"StopNetwork: {QMI_ERR.get(err, err)}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dev", default="rmnet_ims")
    ap.add_argument("--mux", type=int, default=8)
    ap.add_argument("--profile", type=int, default=0, help="3GPP profile index (0 = look up by APN)")
    ap.add_argument("--apn", default="ims")
    ap.add_argument("--family", type=int, default=6, choices=(4, 6))
    ap.add_argument("--env", default="/run/imsd.env")
    ap.add_argument("--start-timeout", type=int, default=90)
    ap.add_argument("--probe", type=float, default=None,
                    help="probe only: start, report, hold N s, stop; no netdev/env changes")
    args = ap.parse_args()

    if not args.profile and args.apn:
        args.profile = lookup_profile(args.apn)
        if args.profile:
            log(f"ims profile index {args.profile} (APN {args.apn})")
        else:
            log(f"no 3GPP profile with APN {args.apn}; APN string only")

    w = Wds()
    if args.probe is None:
        ensure_link(args.dev, args.mux)
    handle, st = start(w, args)
    summary = (f"settings: ipv6 address {'yes' if 'addr' in st else 'no'} prefix /{st.get('prefix')}, "
               f"mtu {st.get('mtu')}, pco-pcscf-flag {st.get('pco')}, ipv6 P-CSCFs {len(st['pcscf'])}, "
               f"ipv4 P-CSCFs {st['pcscf_v4']}")
    log(summary)
    if args.probe is not None:
        time.sleep(args.probe)
        stop(w, handle)
        return 0
    if "addr" not in st:
        log("no IPv6 address on the IMS call")
        stop(w, handle)
        return 1

    stopping = []
    signal.signal(signal.SIGTERM, lambda *_: stopping.append(1))
    signal.signal(signal.SIGINT, lambda *_: stopping.append(1))
    try:
        configure(args.dev, st, args.env)
    except Exception as e:  # noqa: BLE001
        log(f"netdev setup failed: {e}")
        stop(w, handle)
        return 1
    sd_notify("READY=1\nSTATUS=IMS PDN up")
    rc = 0
    while not stopping:
        try:
            ind = w.indication(5)
        except InterruptedError:
            continue
        if ind and ind[0] == PKT_STATUS:
            tl = ind[1]
            status = tl.get(0x01, b"\0")[0]
            if status != 2:  # 1 disconnected, 2 connected, 3 suspended, 4 authenticating
                log(f"IMS call state {status} ({end_reason(tl)}); exiting for a restart")
                rc = 1
                break
        if not os.path.exists(f"/sys/class/net/{args.dev}"):
            log(f"{args.dev} vanished (oFono restart or modem SSR); exiting for a restart")
            rc = 1
            break
    if stopping or rc:
        stop(w, handle)
    return rc


def lookup_profile(apn):
    try:
        out = subprocess.run(["timeout", "-k", "5", "30", "qmicli", "-d", "qrtr://0",
                              "--wds-get-profile-list=3gpp"],
                             capture_output=True, text=True).stdout
    except OSError:
        return 0
    idx = 0
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("[") and "] 3gpp" in line:
            try:
                idx = int(line[1:line.index("]")])
            except ValueError:
                idx = 0
        elif line.startswith("APN:") and idx:
            if line.split("'")[1:2] and line.split("'")[1].lower() == apn.lower():
                return idx
    return 0


if __name__ == "__main__":
    sys.exit(main())
