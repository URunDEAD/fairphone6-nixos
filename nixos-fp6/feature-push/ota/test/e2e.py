#!/usr/bin/env python3
"""Laptop end-to-end dry run of the OTA path, with Settings page previews.

  nix-build test/qt-env.nix -o /tmp/fp6-ota-qt
  test/e2e.py /tmp/fp6-ota-qt OUTDIR [shot names...]

Real: fp6-release (keygen, publish into a file:// binary cache signed with a
fresh test key, manifest signing, its HTTP server on 127.0.0.1), fp6-updated
(the D-Bus service on a private session bus, the health check as a
separate process), nix copy into a SEPARATE chroot store (--store
/tmp/fp6-ota-e2e-*/phone) with signatures enforced, nix-env profiles, the
Settings page (qmlscene, offscreen, 1116x2412 at GRID_UNIT_PX=24).
Fake: the phone root is a temp dir; boot_a/boot_b and the *_a/*_b pairs are
regular files behind a by-partlabel dir; slot control is a JSON file with
qbootctl's transitions; health facts, network/charging conditions and the
night-window hour are files; reboots are logged. Nothing here touches a
device: the daemon refuses non-boot labels and only accepts regular files
because the config has a "fake" section.
"""
import json, os, shutil, subprocess, sys, tempfile, time, hashlib

qt = os.path.abspath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
only = sys.argv[3:]
here = os.path.dirname(os.path.abspath(__file__))
os.makedirs(out, exist_ok=True)
qmlscene = f"{qt}/bin/fp6-qmlscene"
envsh = open(qmlscene).read()
core = envsh.split("FP6_OTA=")[1].split("\n")[0].strip()
plugin = envsh.split("FP6_LSS_PLUGIN=")[1].split("\n")[0].strip()
UPD, REL = f"{core}/bin/fp6-updated", f"{core}/bin/fp6-release"
T = tempfile.mkdtemp(prefix="fp6-ota-e2e-", dir="/tmp")
FP = f"{T}/fakephone"
PHONE_STORE = f"{T}/phone"
SRV = f"{T}/srv"
BOOTSZ = 1 << 20
log = open(f"{T}/e2e.log", "w")
procs = []


def check(cond, msg):
    if not cond:
        raise SystemExit("CHECK FAILED: " + msg)
    print("ok:", msg, flush=True)


def sh(*a, ok=True, env=None):
    r = subprocess.run(list(a), capture_output=True, text=True, env=env)
    log.write(f"$ {' '.join(a)}\n{r.stdout}{r.stderr}\n")
    log.flush()
    if ok and r.returncode:
        raise RuntimeError(f"{a[0]} {a[1] if len(a) > 1 else ''} failed: {r.stderr.strip()[-600:]}")
    return r


def build(release):
    o = sh("nix-build", "--no-out-link", f"{here}/e2e-systems.nix", "-A", "toplevel", "-A", "bootimg", "--arg", "release", str(release)).stdout.split()
    return o[0], o[1]


def sha(b):
    return hashlib.sha256(b).hexdigest()


def wjson(p, v):
    with open(p + ".tmp", "w") as f:
        json.dump(v, f)
    os.replace(p + ".tmp", p)


# ------------------------------------------------------------------ setup
systems = {r: build(r) for r in (1, 2, 3)}
check(all(os.path.exists(t) and os.path.exists(b) for t, b in systems.values()), "fake systems r1-r3 built (toplevel + boot image)")
sh(REL, "keygen", "--dir", f"{T}/keys")
pub_b64 = open(f"{T}/keys/manifest.pub").read().splitlines()[1]
nix_pub = open(f"{T}/keys/nix-cache.pub").read().strip()
check(oct(os.stat(f"{T}/keys/manifest.key").st_mode & 0o777) == "0o600", "manifest secret key is 0600")

# phone store: r1 is "installed" (copied without signature check, like the first manual install)
sh("nix", "--extra-experimental-features", "nix-command", "copy", "--no-check-sigs", "--to", PHONE_STORE, *systems[1])

def fresh_phone(ab):
    shutil.rmtree(FP, ignore_errors=True)
    for d in ["dev", "by-partlabel", "state", "profiles", "gcroots"]:
        os.makedirs(f"{FP}/{d}")
    img1 = open(systems[1][1], "rb").read()
    parts = {
        "boot_a": b"STOCK-ANDROID-BOOT" * 100, "boot_b": img1,
        "abl_a": b"abl-v1" * 1000, "abl_b": b"abl-v1" * 1000,
        "modem_a": b"modem-v1" * 4000, "modem_b": b"modem-v1" * 4000,
        "vbmeta_a": b"AVB0" + b"\0" * 252, "vbmeta_b": b"AVB0" + b"\0" * 252,
    }
    for i, (label, data) in enumerate(parts.items()):
        if label.startswith("boot_"):
            data = data + b"\xab" * (BOOTSZ - len(data))
        dev = f"{FP}/dev/sde{i + 1}"
        open(dev, "wb").write(data)
        os.symlink(dev, f"{FP}/by-partlabel/{label}")
    open(f"{FP}/persist", "wb").write(b"persist")
    os.symlink(f"{FP}/persist", f"{FP}/by-partlabel/persist")
    boot_as(1, "_b")
    wjson(f"{FP}/slots.json", {"current": None, "slots": [
        {"suffix": "_a", "active": False, "successful": False, "bootable": True},
        {"suffix": "_b", "active": True, "successful": True, "bootable": True}]})
    conditions(network="wifi", charging=False)
    healthy(True)
    open(f"{FP}/hour", "w").write("12\n")
    cfg = {
        "uid": os.getuid(), "polkitAction": None, "sessionBus": True, "device": "fairphone-fp6", "channel": "test",
        "server": None, "manifestPublicKey": pub_b64, "nixPublicKey": nix_pub, "allowSlotSwitch": ab, "maxBootAttempts": 2,
        "bootPartitionSize": BOOTSZ, "bootedRelease": f"{FP}/booted-system/etc/fp6-release",
        "currentRelease": f"{FP}/booted-system/etc/fp6-release", "bootedSystem": f"{FP}/booted-system",
        "byPartlabel": f"{FP}/by-partlabel", "procCmdline": f"{FP}/cmdline", "stateDir": f"{FP}/state",
        "backupDir": f"{FP}/state/backups", "profiles": f"{FP}/profiles", "profile": "system", "gcroots": f"{FP}/gcroots",
        "storeDir": f"{PHONE_STORE}/nix/store", "nixStore": PHONE_STORE, "nix": "nix", "nixEnv": "nix-env",
        "fake": {"conditions": f"{FP}/conditions.json", "healthFacts": f"{FP}/health.json", "slots": f"{FP}/slots.json",
                 "rebootLog": f"{FP}/reboot.log", "hourFile": f"{FP}/hour"},
        "idleExit": 0,
    }
    wjson(f"{FP}/config.json", cfg)


def boot_as(release, slot):
    """Simulate the phone (re)booting into `release` from `slot`."""
    link = f"{FP}/booted-system"
    if os.path.islink(link):
        os.remove(link)
    os.symlink(systems[release][0], link)
    open(f"{FP}/cmdline", "w").write(f"console=tty1 quiet androidboot.slot_suffix={slot} androidboot.verifiedbootstate=orange\n")


def conditions(**kw):
    c = {"network": "wifi", "metered": False, "charging": False, "saver": False, "battery": 80}
    c.update(kw)
    wjson(f"{FP}/conditions.json", c)


def healthy(ok):
    wjson(f"{FP}/health.json", {
        "displayManagerActive": ok, "shellProcesses": ["lomiri"] if ok else [], "shellStable": ok,
        "failedCriticalUnits": [] if ok else ["display-manager.service"], "failedUnits": [], "simPresent": True,
        "modemPowered": True, "modemOnline": True, "sshdActive": True, "sshListening": True,
        "netbirdActive": True, "netbirdConnected": False})


env = dict(os.environ)
for k in ["QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE", "DBUS_SESSION_BUS_ADDRESS"]:
    env.pop(k, None)
bus = f"fp6ota-e2e-{os.getpid()}"
env.update({"DBUS_SESSION_BUS_ADDRESS": f"unix:abstract={bus}", "FP6_UPDATE_SESSION_BUS": "1", "TZ": "Europe/Bucharest",
            "LANG": "en_US.UTF-8", "HOME": f"{T}/home", "XDG_CONFIG_HOME": f"{T}/home/.config", "XDG_CACHE_HOME": f"{T}/home/.cache"})
os.makedirs(f"{T}/home/.config", exist_ok=True)
with open(f"{T}/bus.conf", "w") as f:
    f.write(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:abstract={bus}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>""")
procs.append(subprocess.Popen(["dbus-daemon", "--config-file", f"{T}/bus.conf", "--nofork"], env=env))
port = 18742 + os.getpid() % 1000
os.makedirs(SRV, exist_ok=True)
def iface_ip(name):
    r = subprocess.run(["ip", "-4", "-o", "addr", "show", name], capture_output=True, text=True)
    f = r.stdout.split()
    return f[3].split("/")[0] if len(f) > 3 else None

# The laptop's own NetBird address (detected, never written down), else none.
nb_ip = iface_ip("wt0")
listen = f"127.0.0.1:{port}" + (f",{nb_ip}:{port}" if nb_ip else "")
procs.append(subprocess.Popen([REL, "serve", "--root", SRV, "--listen", listen], env=env, stderr=open(f"{T}/server.log", "w")))
time.sleep(0.5)
daemon = None


def start_daemon():
    global daemon
    if daemon:
        daemon.terminate()
        daemon.wait(10)
    daemon = subprocess.Popen([UPD, "--config", f"{FP}/config.json", "serve"], env=env, stderr=open(f"{T}/daemon.log", "a"))
    for _ in range(50):
        if sh(UPD, "--config", f"{FP}/config.json", "call", "status", ok=False, env=env).returncode == 0:
            return
        time.sleep(0.1)
    raise RuntimeError("daemon did not start")


def call(method, args=None, ok=True):
    r = sh(UPD, "--config", f"{FP}/config.json", "call", method, json.dumps(args or {}), ok=False, env=env)
    if r.returncode:
        if ok:
            raise RuntimeError(f"{method}: {r.stderr.strip()}")
        return {"error": r.stderr.strip()}
    return json.loads(r.stdout)


def wait_job(op=None):
    for _ in range(1200):
        st = call("status")
        j = st["job"]
        if not st["busy"] and j and j.get("state") != "running" and (op is None or j["op"] == op):
            return j
        time.sleep(0.25)
    raise RuntimeError("job timeout")


def job(method, args=None):
    call(method, args)
    return wait_job(method)


def health():
    r = sh(UPD, "--config", f"{FP}/config.json", "health-check", ok=False, env=env)
    return json.loads(r.stdout) if r.returncode == 0 else {"error": r.stderr.strip()}


def publish(release, changelog):
    t, b = systems[release]
    sh(REL, "publish", "--root", SRV, "--channel", "test", "--toplevel", t, "--boot-image", b, "--keys", f"{T}/keys",
       "--changelog", changelog, "--kernel-notes", "Kernel 7.2.0-fp6-nixos6 (DTB unchanged); needs firmware from slot B.")


def part(label):
    return open(os.path.realpath(f"{FP}/by-partlabel/{label}"), "rb").read()


def slots():
    return {s["suffix"]: s for s in json.load(open(f"{FP}/slots.json"))["slots"]}


def reboots():
    return open(f"{FP}/reboot.log").read().count("reboot") if os.path.exists(f"{FP}/reboot.log") else 0


HARNESS = """import QtQuick 2.12
import Lomiri.Components 1.3
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri-system-settings"
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background }
    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("%PAGE%")
    }
    function find(item, name) {
        if (!item) return null
        if (item.objectName === name) return item
        var ch = item.children || []
        for (var i = 0; i < ch.length; i++) { var r = find(ch[i], name); if (r) return r }
        if (item.contentItem && item.contentItem !== item) { var c = find(item.contentItem, name); if (c) return c }
        return null
    }
    function el(name) { var r = find(mv, name); if (!r) console.log("HARNESS: no " + name); return r }
    function top() { return pageStack.primaryPage }
    function scroll(name, y) { var p = el(name); if (p && p.flickable) p.flickable.contentY = y }
    function scrollEnd(name) { var p = el(name); if (p && p.flickable) p.flickable.contentY = Math.max(0, p.flickable.contentHeight - p.flickable.height) }
    property var steps: [ %STEPS% ]
    property int step: 0
    Timer {
        interval: %DELAY%
        repeat: true
        running: true
        onTriggered: {
            if (mv.step < mv.steps.length) { var f = mv.steps[mv.step]; mv.step++; try { f() } catch (e) { console.log("HARNESS: step failed: " + e) } }
            else { running = false; mv.grabToImage(function(r) { r.saveToFile("%OUT%"); Qt.quit() }) }
        }
    }
}
"""


def shot(name, steps=(), delay=1500, theme="Ambiance"):
    if only and name not in only:
        return
    src = f"{plugin}/share/lomiri-system-settings/qml-plugins/update/PageComponent.qml"
    qml = HARNESS.replace("%PAGE%", "file://" + src).replace("%DELAY%", str(delay)).replace("%OUT%", f"{out}/{name}.png") \
        .replace("%STEPS%", ", ".join(f"function() {{ {s} }}" for s in steps))
    path = f"{T}/harness-{name}.qml"
    open(path, "w").write(qml)
    os.makedirs(f"{T}/home/.config/lomiri-ui-toolkit", exist_ok=True)
    open(f"{T}/home/.config/lomiri-ui-toolkit/theme.ini", "w").write(f"[General]\ntheme=Lomiri.Components.Themes.{theme}\n")
    e = dict(env, QT_QPA_PLATFORM="offscreen", GRID_UNIT_PX="24")
    r = subprocess.run([qmlscene, path], env=e, capture_output=True, text=True, timeout=90)
    lg = "\n".join(l for l in (r.stdout + r.stderr).splitlines() if "fontconfig" not in l.lower())
    bad = [l for l in lg.splitlines() if any(k in l for k in ("Error", "error", "HARNESS", "TypeError", "ReferenceError", "is not defined", "Cannot"))
           and "Binding loop" not in l]
    open(f"{T}/{name}.log", "w").write(lg)
    print(f"shot {name}: {'saved' if os.path.exists(out + '/' + name + '.png') else 'MISSING'}" + ("" if not bad else "\n  " + "\n  ".join(bad[:12])), flush=True)


try:
    # ================================================= single-slot (default)
    fresh_phone(ab=False)
    start_daemon()
    st = call("status")
    check(st["running"]["release"] == 1 and not st["abMode"] and st["slot"] == "_b", "phone runs release 1 on slot _b, single-slot mode")
    shot("01-no-server")
    j = job("check")
    check(j["state"] == "failed" and "No update server" in j["error"], "check without a server says so")
    # the server is set through the page: type, Save
    shot("01b-server-invalid", ["el('fp6UpdateServerField').text = 'http://8.8.8.8:8742'", "scrollEnd('fp6UpdatePage')"], delay=1000)
    shot("01c-server-saved", [f"el('fp6UpdateServerField').text = 'http://127.0.0.1:{port}/'", "el('fp6UpdateServerSave').clicked()",
                              "0", "scrollEnd('fp6UpdatePage')"], delay=1200)
    if not only:
        check(call("status")["server"] == f"http://127.0.0.1:{port}", "server saved from the page (normalised, no trailing slash)")
    else:
        call("setServer", {"server": f"http://127.0.0.1:{port}"})
    for bad in ["http://example.org", "http://8.8.8.8:8742", "ftp://10.0.0.1", "http://user:pw@10.0.0.1", "http://10.0.0.1/?x"]:
        check("error" in call("setServer", {"server": bad}, ok=False), f"setServer refuses {bad}")
    check(call("status")["server"] == f"http://127.0.0.1:{port}", "a refused URL leaves the saved server unchanged")
    t = call("testServer", {"server": f"http://127.0.0.1:{port + 1}"})
    check(not t["ok"], f"Test connection to a dead port fails: {t['error'][:80]}")
    publish(1, "First release.")
    t = call("testServer", {"server": f"http://127.0.0.1:{port}"})
    check(t["ok"] and t["release"] == 1 and not t["newer"] and t["keyId"], f"Test connection verifies the manifest (key {t.get('keyId')})")
    if nb_ip:
        nb = f"http://{nb_ip}:{port}"
        shot("02b-server-test", [f"el('fp6UpdateServerField').text = '{nb}'", "el('fp6UpdateServerTest').clicked()", "0", "0",
                                 "scrollEnd('fp6UpdatePage')"], delay=1200)
        call("setServer", {"server": nb})
        check(call("status")["server"] == nb, "server switched to the laptop's NetBird address")
        t = call("testServer", {})
        check(t["ok"] and t["release"] == 1, "Test connection over NetBird (saved server)")
    else:
        print("note: no wt0 address; NetBird part skipped")
    keys_before = json.load(open(f"{FP}/config.json"))["manifestPublicKey"]
    j = job("check")
    check(j["state"] == "done" and "Up to date" in j["result"]["message"], f"release 1 published: {j['result']['message'] if j['state']=='done' else j['error']}")
    shot("02-up-to-date")
    publish(2, "Settings > System update.\nFaster boot.\nFixes the white boxes in text (GPU preemption off).")
    r = sh("nix", "run", "nixpkgs#minisign", "--", "-Vm", f"{SRV}/channels/test/manifest.json", "-P", pub_b64, ok=False)
    check(r.returncode == 0 and "Signature and comment signature verified" in r.stdout + r.stderr, "stock minisign verifies the manifest signature")
    check(sh(REL, "publish", "--root", SRV, "--channel", "test", "--toplevel", systems[2][0], "--boot-image", systems[2][1], "--keys", f"{T}/keys", ok=False).returncode != 0,
          "publishing the same release again is refused")
    check(sh(REL, "publish", "--root", SRV, "--channel", "test", "--toplevel", systems[3][0], "--boot-image", systems[2][1], "--keys", f"{T}/keys", ok=False).returncode != 0,
          "a boot image that pins another toplevel is refused")

    # tampering: changed manifest, swapped signature, closure list changed
    mf = f"{SRV}/channels/test/manifest.json"
    good = open(mf, "rb").read()
    open(mf, "wb").write(good.replace(b"Faster boot", b"Faster boat"))
    j = job("check")
    check(j["state"] == "failed" and "signature" in j["error"], f"tampered manifest rejected: {j['error']}")
    open(mf, "wb").write(good)
    cf = f"{SRV}/channels/test/closure-r2.json"
    goodc = open(cf, "rb").read()
    open(cf, "wb").write(goodc.replace(b'"narSize":', b'"narSize": ', 1))
    j = job("check")
    check(j["state"] == "failed" and "closure list does not match" in j["error"], "changed closure list rejected")
    open(cf, "wb").write(goodc)
    sig = f"{mf}.minisig"
    goods = open(sig).read()
    shutil.copy(f"{SRV}/channels/test/releases/1/manifest.json.minisig", sig)
    j = job("check")
    check(j["state"] == "failed", f"signature of release 1 replayed onto release 2 rejected: {j['error']}")
    open(sig, "w").write(goods)

    j = job("check")
    st = call("status")
    av = st["available"]
    check(j["state"] == "done" and av and av["release"] == 2 and av["downloadBytes"] > 0 and not av["downloaded"],
          f"release 2 offered, download {av and av['downloadBytes']} B")
    shot("03-available")

    # store signatures: an unsigned cache is refused by the same nix copy options
    sh("nix", "--extra-experimental-features", "nix-command", "copy", "--to", f"file://{T}/unsigned-cache", *systems[2])
    r = sh("nix", "--extra-experimental-features", "nix-command", "copy", "--from", f"file://{T}/unsigned-cache", "--to", f"{T}/other-store",
           "--option", "trusted-public-keys", nix_pub, systems[2][0], ok=False)
    check(r.returncode != 0 and "signature" in r.stderr.lower(), "nix copy from an unsigned cache fails (no valid signature)")

    # the same over HTTP: a cache whose narinfos lost their signatures is refused
    shutil.copytree(SRV, f"{T}/srv-nosig")
    for f in os.listdir(f"{T}/srv-nosig/cache"):
        if f.endswith(".narinfo"):
            p = f"{T}/srv-nosig/cache/{f}"
            kept = "".join(l for l in open(p) if not l.startswith("Sig:"))
            open(p, "w").write(kept)
    port2 = port + 2
    procs.append(subprocess.Popen([REL, "serve", "--root", f"{T}/srv-nosig", "--listen", f"127.0.0.1:{port2}"], env=env,
                                  stderr=open(f"{T}/server-nosig.log", "w")))
    time.sleep(0.5)
    r = sh("nix", "--extra-experimental-features", "nix-command", "copy", "--from", f"http://127.0.0.1:{port2}/cache", "--to", f"{T}/other-store2",
           "--option", "trusted-public-keys", nix_pub, systems[2][0], ok=False)
    check(r.returncode != 0 and "signature" in r.stderr.lower(), "nix copy --from http:// with stripped signatures fails")
    r = sh("nix", "--extra-experimental-features", "nix-command", "copy", "--from", f"http://127.0.0.1:{port}/cache", "--to", f"{T}/other-store3",
           "--option", "trusted-public-keys", nix_pub, systems[2][0], ok=False)
    check(r.returncode == 0, "nix copy --from http:// of the signed cache works with only the release key trusted")
    check(json.load(open(f"{FP}/config.json"))["manifestPublicKey"] == keys_before and call("status")["keyId"],
          "server changes never touched the trusted key")

    conditions(network="cellular")
    j = job("download")
    check(j["state"] == "failed" and "Not on Wi-Fi" in j["error"], "download refused on cellular (Wi-Fi only)")
    shot("04-not-on-wifi")
    conditions(network="wifi", saver=True)
    j = job("download")
    check(j["state"] == "failed" and "Battery saver" in j["error"], "download refused in battery saver")
    conditions(network="wifi")
    j = job("download")
    check(j["state"] == "done", f"downloaded: {j.get('error')}")
    check(all(os.path.exists(f"{PHONE_STORE}{p}") for p in systems[2]), "toplevel and boot image are in the phone's separate store")
    check(sorted(os.listdir(f"{FP}/gcroots")) == ["pending-boot-image", "pending-toplevel"], "GC roots for the download")
    st = call("status")
    check(st["available"]["downloaded"], "marked downloaded")
    shot("05-downloaded")

    before_b = part("boot_b")
    check("error" in call("install", {"when": "now"}, ok=False), "install without confirmation refused")
    j = job("install", {"when": "now", "confirm": True})
    check(j["state"] == "done" and j["result"]["partition"] == "boot_b", f"installed to boot_b: {j.get('error')}")
    img2 = open(systems[2][1], "rb").read()
    after_b = part("boot_b")
    check(after_b[:len(img2)] == img2 and len(after_b) == BOOTSZ and after_b[-1] == 0xab, "boot_b = r2 image, no truncation, tail untouched")
    check(part("boot_a").startswith(b"STOCK"), "boot_a untouched")
    check(part("persist") == b"persist", "persist untouched")
    bks = sorted(os.listdir(f"{FP}/state/backups"))
    full = [b for b in bks if not b.endswith(".trimmed.img")]
    check(len(full) == 1 and open(f"{FP}/state/backups/{full[0]}", "rb").read() == before_b, "full boot_b backup equals the old partition")
    img1 = open(systems[1][1], "rb").read()
    trimmed = [b for b in bks if b.endswith(".trimmed.img")]
    check(len(trimmed) == 1 and open(f"{FP}/state/backups/{trimmed[0]}", "rb").read() == img1, "trimmed backup = the r1 image (fastboot-bootable size)")
    check(os.path.realpath(f"{FP}/profiles/system").endswith(os.path.basename(systems[2][0])) or
          os.readlink(f"{FP}/profiles/system").startswith("system-"), "system profile set")
    prof = os.readlink(f"{FP}/profiles/" + os.readlink(f"{FP}/profiles/system"))
    check(prof == systems[2][0], f"profile generation -> r2 toplevel ({prof})")
    st = call("status")
    check(st["pending"]["mode"] == "single" and st["pending"]["release"] == 2, "pending restart (single-slot)")
    shot("06-installed-restart")
    shot("07-confirm-restart", ["el('fp6UpdateRestart').clicked()"], delay=1200)

    # undo before the restart
    j = job("rollback", {"confirm": True})
    check(j["state"] == "done" and j["result"].get("cancelled"), "undo before restart")
    check(part("boot_b") == before_b, "boot_b restored byte for byte")
    check(call("status")["pending"] is None, "nothing pending after undo")
    j = job("install", {"when": "now", "confirm": True})
    check(j["state"] == "done", "installed again")
    n = reboots()
    call("reboot", {"confirm": True})
    check(reboots() == n + 1, "restart requested (logged)")
    boot_as(2, "_b")
    h = health()
    check(h.get("passed") and h["action"] == "MarkSuccessful", f"start check on r2: {h}")
    st = call("status")
    check(st["pending"] is None and st["history"][0]["outcome"] == "confirmed" and st["running"]["release"] == 2, "release 2 confirmed")
    check(st["available"] is None, "nothing left to offer")
    check(os.listdir(f"{FP}/gcroots") == ["previous-toplevel"], "pending GC roots dropped, previous system kept")
    shot("08-updated")
    shot("09-confirm-rollback", ["el('fp6UpdateRollback').clicked()"], delay=1200)

    j = job("rollback", {"confirm": True})
    check(j["state"] == "done", f"rollback (single-slot): {j.get('error')}")
    check(part("boot_b") == before_b, "boot_b holds the r1 partition contents again")
    prof = os.readlink(f"{FP}/profiles/" + os.readlink(f"{FP}/profiles/system"))
    check(prof == systems[1][0], "profile -> r1")
    boot_as(1, "_b")
    h = health()
    check(h["action"] == "MarkSuccessful", "rollback confirmed by the start check")
    healthy(False)
    h = health()
    check(not h["passed"] and h["action"] == "Record", "an unhealthy boot without a pending update is only recorded")
    shot("10-health-failed")
    healthy(True)
    health()

    # ================================================= A/B (allowSlotSwitch = true, fake slots)
    fresh_phone(ab=True)
    start_daemon()
    call("setServer", {"server": f"http://{nb_ip or '127.0.0.1'}:{port}"})
    publish(3, "Release 3 (A/B test).")
    j = job("check")
    check(call("status")["available"]["release"] == 3, "release 3 offered")
    job("download")
    check(call("status")["available"]["downloaded"], "release 3 downloaded")
    shot("11-ab-ready")
    # firmware differs between slots: refused before anything is written
    open(os.path.realpath(f"{FP}/by-partlabel/modem_a"), "wb").write(b"modem-OLD" * 4000)
    a_before = part("boot_a")
    j = job("install", {"when": "now", "confirm": True})
    check(j["state"] == "failed" and "not interchangeable" in j["error"] and "modem" in j["error"], f"A/B install refused: {j['error']}")
    check(part("boot_a") == a_before and slots()["_b"]["active"], "boot_a and the slots untouched")
    r = sh(UPD, "--config", f"{FP}/config.json", "pairs", env=env)
    v = json.loads(r.stdout)["verdict"]
    check(v["differing"] == ["modem"] and not v["safe"], "pairs report: modem differs")
    shot("12-ab-refused")
    open(os.path.realpath(f"{FP}/by-partlabel/modem_a"), "wb").write(b"modem-v1" * 4000)

    j = job("install", {"when": "now", "confirm": True})
    img3 = open(systems[3][1], "rb").read()
    check(j["state"] == "done" and j["result"]["partition"] == "boot_a", f"installed to the inactive boot_a: {j.get('error')}")
    check(part("boot_a")[:len(img3)] == img3 and part("boot_b")[:len(img1)] == img1, "boot_a = r3, boot_b still r1")
    s = slots()
    check(s["_a"]["active"] and not s["_a"]["successful"] and not s["_b"]["active"] and s["_b"]["successful"], "slot _a active, not yet successful")
    shot("13-ab-installed")
    # two failing boots: retry, then rollback
    boot_as(3, "_a")
    healthy(False)
    n = reboots()
    h = health()
    check(h["action"] == "Retry { attempt: 1 }" and reboots() == n + 1 and slots()["_a"]["active"], "1st failed start check: slot re-armed, reboot")
    h = health()
    check(h["action"] == "RollBack" and slots()["_b"]["active"] and reboots() == n + 2, "2nd failed start check: slot _b active again, reboot")
    boot_as(1, "_b")
    healthy(True)
    h = health()
    st = call("status")
    check(h["action"] == "Record" and st["history"][0]["outcome"] == "failed", "back on r1; history says failed")
    shot("14-ab-failed")
    # bootloader fallback (the new system never reached the start check)
    j = job("install", {"when": "now", "confirm": True})
    check(j["state"] == "done", "installed again")
    # ABL: retry count exhausted -> _a unbootable, _b active
    wjson(f"{FP}/slots.json", {"current": None, "slots": [
        {"suffix": "_a", "active": False, "successful": False, "bootable": False},
        {"suffix": "_b", "active": True, "successful": True, "bootable": True}]})
    boot_as(1, "_b")
    h = health()
    check(h["action"] == "FellBack" and call("status")["history"][0]["outcome"] == "rolled-back", "bootloader fallback recorded")
    # the same, but the GPT still names _a: the daemon makes the running slot active again
    j = job("install", {"when": "now", "confirm": True})
    check(j["state"] == "done", f"installed again ({j.get('error')})")
    boot_as(1, "_b")
    h = health()
    check(h["action"] == "FellBack" and slots()["_b"]["active"] and slots()["_b"]["successful"], "fallback with a stale active flag repaired")
    # success
    j = job("install", {"when": "now", "confirm": True})
    boot_as(3, "_a")
    h = health()
    s = slots()
    check(h["action"] == "MarkSuccessful" and s["_a"]["successful"] and s["_a"]["active"], "healthy r3 on _a marked successful")
    shot("15-ab-updated")
    # rollback to slot _b
    j = job("rollback", {"confirm": True})
    check(j["state"] == "done" and slots()["_b"]["active"], f"A/B rollback: slot _b active ({j.get('error')})")
    boot_as(1, "_b")
    h = health()
    check(h["action"] == "MarkSuccessful" and slots()["_b"]["successful"], "rollback confirmed")
    # install tonight
    j = job("check")
    check(call("status")["available"]["release"] == 3, "release 3 offered again")
    call("install", {"when": "tonight", "confirm": True})
    shot("16-tonight")
    conditions(network="wifi", charging=True)
    job("scheduled")
    check(call("status")["pending"] is None, "not installed at 12:00")
    open(f"{FP}/hour", "w").write("3\n")
    n = reboots()
    j = job("scheduled")
    st = call("status")
    check(st["pending"] and st["pending"]["release"] == 3 and reboots() == n + 1, f"installed at 03:00 while charging, then restarted ({j.get('result')})")
    shot("17-dark", theme="SuruDark")
finally:
    for p in ([daemon] if daemon else []) + list(reversed(procs)):
        p.terminate()
    for p in ([daemon] if daemon else []) + procs:
        try:
            p.wait(5)
        except Exception:
            p.kill()
    print("work dir (logs, fake phone, stores):", T)
