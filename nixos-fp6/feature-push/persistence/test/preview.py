#!/usr/bin/env python3
"""Laptop (x86) previews of Settings > VPN > Reconnect at startup, offscreen.

  nix-build test/qt-env.nix -o /tmp/fp6-vpn-qt
  test/preview.py /tmp/fp6-vpn-qt OUTDIR

The stock VPN PageComponent.qml with lss-vpn-startup.patch applied and
Fp6VpnStartup.qml next to it, rendered at the FP6's 1116x2412
(GRID_UNIT_PX=24). GSettings uses the keyfile backend, pre-filled with what
fp6-vpn-restore publishes. A tap scenario checks that choosing a mode writes
startup-modes (and that "restore", the default, is not stored).
"""
import os, shutil, subprocess, sys, tempfile

qt = os.path.abspath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
here = os.path.dirname(os.path.abspath(__file__))
os.makedirs(out, exist_ok=True)
scratch = tempfile.mkdtemp(prefix="fp6-vpn-preview-")
qmlscene = f"{qt}/bin/fp6-qmlscene"
lss = open(qmlscene).read().split("FP6_LSS=")[1].split("\n")[0].strip()

page_dir = f"{scratch}/vpn"
shutil.copytree(f"{lss}/share/lomiri-system-settings/qml-plugins/vpn", page_dir)
os.chmod(page_dir, 0o755)
for f in os.listdir(page_dir):
    os.chmod(f"{page_dir}/{f}", 0o644)
subprocess.run(["patch", "-s", "-p1", "-d", page_dir, "-i", f"{here}/../lss-vpn-startup.patch"], check=True)
shutil.copy(f"{here}/../settings-page/Fp6VpnStartup.qml", f"{page_dir}/Fp6VpnStartup.qml")

home = f"{scratch}/home"
env = dict(os.environ)
for k in ["QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE", "DBUS_SESSION_BUS_ADDRESS"]:
    env.pop(k, None)
env.update({"HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "LANG": "en_US.UTF-8",
            "QT_QPA_PLATFORM": "offscreen", "GRID_UNIT_PX": "24",
            # No session bus: the stock list (connectivity-service) stays empty.
            "DBUS_SESSION_BUS_ADDRESS": "unix:path=/nonexistent"})
keyfile = f"{home}/.config/glib-2.0/settings/keyfile"
os.makedirs(os.path.dirname(keyfile), exist_ok=True)
os.makedirs(f"{home}/.config/lomiri-ui-toolkit", exist_ok=True)

NB = "6e3c1f0a-9b7d-4c55-8f0e-0b1d5e7a4b21"
WORK = "0f0e0d0c-0b0a-4909-8807-060504030201"

HARNESS = """import QtQuick 2.12
import Lomiri.Components 1.3
import GSettings 1.0
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri-system-settings"
    backgroundColor: theme.palette.normal.background
    property bool showAllUI: false
    property bool isSnap: false
    GSettings { id: check; schema.id: "com.fairphone6.vpn" }
    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("file://%PAGE%")
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
    property var steps: [ %STEPS% ]
    property int step: 0
    Timer {
        interval: 1200
        repeat: true
        running: true
        onTriggered: {
            if (mv.step < mv.steps.length) { var f = mv.steps[mv.step]; mv.step++; try { f() } catch (e) { console.log("HARNESS: step failed: " + e) } }
            else { running = false; mv.grabToImage(function(r) { r.saveToFile("%OUT%"); Qt.quit() }) }
        }
    }
}
"""

failures = []


def gv(lines):
    return "[" + ", ".join("'" + l.replace("\t", "\\t") + "'" for l in lines) + "]"


def shot(name, connections, modes=(), steps=(), theme="Ambiance"):
    with open(keyfile, "w") as f:
        f.write("[com/fairphone6/vpn]\n")
        f.write(f"connections={gv(connections)}\n")
        f.write(f"startup-modes={gv(modes)}\n")
    open(f"{home}/.config/lomiri-ui-toolkit/theme.ini", "w").write(f"[General]\ntheme=Lomiri.Components.Themes.{theme}\n")
    qml = HARNESS.replace("%PAGE%", f"{page_dir}/PageComponent.qml").replace("%OUT%", f"{out}/{name}.png") \
        .replace("%STEPS%", ", ".join(f"function() {{ {s} }}" for s in (list(steps) or [""])))
    path = f"{scratch}/harness-{name}.qml"
    open(path, "w").write(qml)
    if os.path.exists(f"{out}/{name}.png"):
        os.unlink(f"{out}/{name}.png")
    r = subprocess.run([qmlscene, path], env=env, capture_output=True, text=True, timeout=90)
    log = "\n".join(l for l in (r.stdout + r.stderr).splitlines() if "fontconfig" not in l.lower())
    open(f"{out}/{name}.log", "w").write(log + "\n")
    bad = [l for l in log.splitlines() if ("rror" in l or "HARNESS" in l or "is not defined" in l or "CHECK" in l
                                            or "Fp6VpnStartup" in l) and "Binding loop" not in l]
    ok = os.path.exists(f"{out}/{name}.png")
    print(f"shot {name}: {'saved' if ok else 'MISSING'}" + ("" if not bad else "\n  " + "\n  ".join(bad[:12])))
    failures.extend(f"{name}: {l}" for l in log.splitlines() if "CHECK FAILED" in l)
    if not ok:
        failures.append(f"{name}: no image")


netbird_on = [f"{NB}\tNetBird\ton"]
two = [f"{NB}\tNetBird\twaiting", f"{WORK}\tWork OpenVPN\toff"]

shot("01-vpn-netbird-restore", netbird_on)
shot("02-vpn-two-always", two, [f"{WORK}\talways"])
shot("03-vpn-none", [], steps=["if (el('fp6VpnStartup').visible) console.log('CHECK FAILED: section shown without VPNs')"])
# Open the selector of NetBird and pick "Always on": startup-modes gets the line.
shot("04-vpn-pick-always", netbird_on, steps=[
    "el('fp6VpnStartup_NetBird').expanded = true", "",
    "var s = el('fp6VpnStartup_NetBird'); s.delegateClicked(1); s.selectedIndex = 1", "",
    "var m = check.startupModes; if (m.length !== 1 || m[0] !== '%s\\talways') console.log('CHECK FAILED: startup-modes ' + JSON.stringify(m))" % NB])
# Back to "Restore last state": the line is dropped again (default).
shot("05-vpn-pick-restore", netbird_on, [f"{NB}\talways"], steps=[
    "var s = el('fp6VpnStartup_NetBird'); if (s.selectedIndex !== 1) console.log('CHECK FAILED: always not selected'); s.delegateClicked(0)", "",
    "var m = check.startupModes; if (m.length !== 0) console.log('CHECK FAILED: startup-modes ' + JSON.stringify(m))"])

shutil.rmtree(scratch, ignore_errors=True)
if failures:
    print("FAILURES:\n  " + "\n  ".join(failures))
    sys.exit(1)
print("all previews OK")
