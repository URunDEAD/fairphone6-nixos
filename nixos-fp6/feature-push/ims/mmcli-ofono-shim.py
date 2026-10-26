"""mmcli (oFono shim): the four ModemManager queries imsd 1.1.0 makes, answered from oFono + QMI.

imsd's daemon reads modem facts by scraping `mmcli` text. On the FP6 Lomiri
image oFono owns the modem and ModemManager is off, so this script is put on
imsd's PATH as `mmcli` and prints exactly the lines imsd's parsers look for
(interfaces/Imsd-Aka.cppm, implementations/main.cpp PacketServiceUp):

  mmcli -m any                 "primary sim path: <oFono modem path>"
                               ("equipment id: <IMEI>" only with FP6_IMS_SEND_IMEI=1)
  mmcli -i <path>              "imsi: <IMSI>", "operator id: <MCC><MNC>"  (SimManager)
  mmcli -m any --location-get  "operator mcc/mnc", "tracking area code", "cell id"
                               (QMI NAS Get System Info, LTE section; hex like mmcli)
  mmcli -m any -K              "modem.generic.state : ..." and
                               "modem.3gpp.packet-service-state : ..."
  mmcli -L                     one fake modem line (for manual checks)

Everything is read-only. Any other invocation exits 2.
"""
import json
import os
import subprocess
import sys


def busctl(path, iface, method="GetProperties"):
    r = subprocess.run(["busctl", "--system", "--json=short", "call", "org.ofono", path,
                        iface, method], capture_output=True, text=True, timeout=10)
    if r.returncode:
        return None
    return json.loads(r.stdout)["data"][0]


def props(path, iface):
    d = busctl(path, f"org.ofono.{iface}")
    return {k: v["data"] for k, v in d.items()} if d else {}


def modem():
    d = busctl("/", "org.ofono.Manager", "GetModems")
    if not d:
        return None, {}
    for path, p in d:
        p = {k: v["data"] for k, v in p.items()}
        if p.get("Powered"):
            return path, p
    return (d[0][0], {k: v["data"] for k, v in d[0][1].items()}) if d else (None, {})


def state_kv(path, mp):
    ifaces = mp.get("Interfaces", [])
    if not mp.get("Powered"):
        st = "disabled"
    elif "org.ofono.SimManager" in ifaces and props(path, "SimManager").get("PinRequired", "none") != "none":
        st = "locked"
    elif not mp.get("Online"):
        st = "disabled"
    elif "org.ofono.NetworkRegistration" not in ifaces:
        st = "enabling"
    else:
        reg = props(path, "NetworkRegistration").get("Status", "unknown")
        st = {"registered": "registered", "roaming": "registered",
              "searching": "searching", "unregistered": "enabled",
              "denied": "enabled"}.get(reg, "enabled")
    ps = "detached"
    if "org.ofono.ConnectionManager" in ifaces and props(path, "ConnectionManager").get("Attached"):
        ps = "attached"
    if st == "registered" and ps == "attached":
        cm_active = any(c[1].get("Active", {}).get("data") for c in
                        (busctl(path, "org.ofono.ConnectionManager", "GetContexts") or []))
        st = "connected" if cm_active else st
    print(f"modem.generic.state                     : {st}")
    print(f"modem.3gpp.packet-service-state         : {ps}")


def location():
    r = subprocess.run(["timeout", "-k", "3", "15", "qmicli", "-d", "qrtr://0", "--nas-get-system-info"],
                       capture_output=True, text=True)
    sec, vals = None, {}
    for line in r.stdout.splitlines():
        s = line.strip()
        if line.startswith("\t") and not line.startswith("\t\t") and s.endswith(":"):
            sec = s[:-1]
            continue
        if sec != "LTE service" or ":" not in s:
            continue
        k, v = s.split(":", 1)
        vals[k.strip()] = v.strip().strip("'")
    try:
        mcc, mnc = vals["MCC"], vals["MNC"]
        tac, ci = int(vals["Tracking Area Code"]), int(vals["Cell ID"])
    except (KeyError, ValueError):
        return
    print("  3GPP location   |     operator mcc: " + mcc)
    print("                  |     operator mnc: " + mnc)
    print(f"                  |     tracking area code: {tac:06X}")
    print(f"                  |     cell id: {ci:08X}")


def main(a):
    path, mp = modem()
    if a[:1] == ["-L"]:
        if path:
            print("    /org/freedesktop/ModemManager1/Modem/0 [oFono] " + path)
        return 0
    if not path:
        print("error: couldn't find modem", file=sys.stderr)
        return 1
    if a == ["-m", "any"]:
        print(f"  SIM      |       primary sim path: {path}")
        if os.environ.get("FP6_IMS_SEND_IMEI") == "1" and mp.get("Serial"):
            print(f"  Hardware |           equipment id: {mp['Serial']}")
        return 0
    if len(a) == 2 and a[0] == "-i":
        sp = props(a[1], "SimManager")
        imsi, mcc, mnc = sp.get("SubscriberIdentity"), sp.get("MobileCountryCode"), sp.get("MobileNetworkCode")
        if not (imsi and mcc and mnc):
            print("error: SIM not ready", file=sys.stderr)
            return 1
        print(f"  Properties |        imsi: {imsi}")
        print(f"             | operator id: {mcc}{mnc}")
        return 0
    if a == ["-m", "any", "--location-get"]:
        location()
        return 0
    if a == ["-m", "any", "-K"]:
        state_kv(path, mp)
        return 0
    print(f"mmcli-ofono-shim: unsupported: {' '.join(a)}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
