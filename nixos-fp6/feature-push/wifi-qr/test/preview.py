#!/usr/bin/env python3
"""Laptop (x86) previews of the Wi-Fi QR pages, offscreen, fake data only.

  nix-build test -A env -o /tmp/fp6-wifiqr-env
  test/preview.py /tmp/fp6-wifiqr-env OUTDIR [shot ...]

Settings pages: the stock Wi-Fi plugin QML (lomiri-system-settings
1.3.2) with lss-wifi-qr.patch applied, plus settings-page/*.qml and the
real Fp6.WifiQr plugin in FP6_WIFIQR_FAKE mode (synthetic network
"Test;Net", password "p@ss:word,1", passcode 1234; no NetworkManager, no
PAM, no D-Bus). Biometryd is a stub reader that never answers.
Scanner: the real fp6-wifi-qr-scan in demo mode: a synthetic QR code is
grabbed and decoded through the app's own QrScanFilter -> Rust path.
Rendered at the FP6's 1116x2412 (GRID_UNIT_PX=24).
"""
import os, shutil, subprocess, sys, tempfile

env_dir = os.path.abspath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
only = sys.argv[3:]
here = os.path.dirname(os.path.abspath(__file__))
src = os.path.dirname(here)
os.makedirs(out, exist_ok=True)
scratch = tempfile.mkdtemp(prefix="fp6-wifiqr-preview-")
qmlscene = f"{env_dir}/bin/fp6-qmlscene"
envsh = open(qmlscene).read()
def var(n):
    return envsh.split(f"export {n}=")[1].split("\n")[0].strip()
lss, plugin, scanner = var("FP6_LSS"), var("FP6_LSS_PLUGIN"), var("FP6_SCANNER")
icons = var("XDG_DATA_DIRS")

# stock Wi-Fi plugin pages + the patch + our pages
wifi = f"{scratch}/wifi"
shutil.copytree(f"{lss}/share/lomiri-system-settings/qml-plugins/wifi", wifi, symlinks=False)
subprocess.run(["chmod", "-R", "u+w", wifi], check=True)
subprocess.run(["patch", "-d", wifi, "-p1", "--forward", "--batch", "-i", f"{src}/lss-wifi-qr.patch"], check=True, stdout=subprocess.DEVNULL)
for f in os.listdir(f"{plugin}/share/lomiri-system-settings/qml-plugins/wifi"):
    s = f"{plugin}/share/lomiri-system-settings/qml-plugins/wifi/{f}"
    (shutil.copytree if os.path.isdir(s) else shutil.copy)(s, f"{wifi}/{f}")

home = f"{scratch}/home"
base_env = dict(os.environ)
for k in ["QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE", "DBUS_SESSION_BUS_ADDRESS",
          "DBUS_SYSTEM_BUS_ADDRESS"]:
    base_env.pop(k, None)
base_env.update({
    "HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "XDG_CACHE_HOME": f"{home}/.cache",
    "XDG_DATA_DIRS": icons, "QT_QPA_PLATFORM": "offscreen", "GRID_UNIT_PX": "24", "LANG": "en_US.UTF-8",
    "FP6_WIFIQR_FAKE": "1", "FP6_EXTRA_QML": f"{here}/stubs",
    # no system/session bus at all in the preview
    "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=/nonexistent", "DBUS_SESSION_BUS_ADDRESS": "unix:path=/nonexistent",
})

HARNESS = """import QtQuick 2.12
import Lomiri.Components 1.3
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri-system-settings"
    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPage: first
    }
    property var first: null
    Component.onCompleted: {
        var c = Qt.createComponent("%PAGE%");
        if (c.status !== Component.Ready) console.log("HARNESS: " + c.errorString());
        first = c.createObject(pageStack, %PROPS%);
        pageStack.primaryPage = first;
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
    function share() { return el("fp6WifiDetailsExtras").share }
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

results = []


def check_log(name, log):
    bad = [l for l in log.splitlines() if any(k in l for k in ("Error", "error:", "HARNESS", "TypeError", "ReferenceError",
                                                             "is not defined", "Cannot assign", "not a type", "failed"))
           and "Binding loop" not in l and "dbus" not in l.lower() and "D-Bus" not in l]
    ok = os.path.exists(f"{out}/{name}.png")
    results.append((name, ok, bad))
    print(f"shot {name}: {'saved' if ok else 'MISSING'}" + ("" if not bad else "\n  " + "\n  ".join(bad[:10])))


def shot(name, page, props="{}", steps=(), delay=900, theme="Ambiance", extra_env=None):
    if only and name not in only:
        return
    qml = HARNESS.replace("%PAGE%", f"file://{wifi}/{page}").replace("%PROPS%", props).replace("%DELAY%", str(delay)) \
        .replace("%OUT%", f"{out}/{name}.png").replace("%STEPS%", ", ".join(f"function() {{ {s} }}" for s in steps))
    path = f"{wifi}/harness-{name}.qml"
    open(path, "w").write(qml)
    os.makedirs(f"{home}/.config/lomiri-ui-toolkit", exist_ok=True)
    open(f"{home}/.config/lomiri-ui-toolkit/theme.ini", "w").write(f"[General]\ntheme=Lomiri.Components.Themes.{theme}\n")
    e = dict(base_env, **(extra_env or {}))
    r = subprocess.run([qmlscene, path], env=e, capture_output=True, text=True, timeout=90)
    log = "\n".join(l for l in (r.stdout + r.stderr).splitlines() if "fontconfig" not in l.lower())
    open(f"{scratch}/{name}.log", "w").write(log)
    check_log(name, log)


def scan(name, step="", payload="", extra_env=None, delay=4500):
    if only and name not in only:
        return
    e = dict(base_env, FP6_PREVIEW="demo", FP6_PREVIEW_STEP=step, FP6_PREVIEW_PAYLOAD=payload,
             FP6_GRAB=f"{out}/{name}.png:{delay}", FP6_GRAB_SIZE="1116x2412", **(extra_env or {}))
    r = subprocess.run([f"{scanner}/bin/fp6-wifi-qr-scan"], env=e, capture_output=True, text=True, timeout=90)
    log = "\n".join(l for l in (r.stdout + r.stderr).splitlines() if "fontconfig" not in l.lower() and "PERFORMANCE" not in l)
    open(f"{scratch}/{name}.log", "w").write(log)
    check_log(name, log)


DETAILS = '{ networkName: "Test;Net", dbusPath: "/fake/1", lastUsed: "27.10.2026" }'
open_share = "el('fp6ShareQr').clicked()"
FP = {"FP6_WIFIQR_FAKE_FINGERPRINT": "1"}

shot("01-wifi-page", "PageComponent.qml")
shot("02-details", "NetworkDetails.qml", DETAILS)
shot("03-confirm-fingerprint", "NetworkDetails.qml", DETAILS, [open_share], extra_env=FP)
shot("04-confirm-wrong-passcode", "NetworkDetails.qml", DETAILS,
     [open_share, "el('fp6WifiAuthField').text = '0000'", "el('fp6WifiAuthConfirm').clicked()"], delay=1200)
shot("05-share-qr", "NetworkDetails.qml", DETAILS,
     [open_share, "el('fp6WifiAuthField').text = '1234'", "el('fp6WifiAuthConfirm').clicked()", ""], delay=1200)
shot("06-share-password-shown", "NetworkDetails.qml", DETAILS,
     [open_share, "el('fp6WifiAuthField').text = '1234'", "el('fp6WifiAuthConfirm').clicked()", "",
      "el('fp6SharePassword').clicked()"], delay=1200)
shot("07-details-password-shown", "NetworkDetails.qml", DETAILS,
     ["el('fp6ShowPasswordSwitch').clicked()", "el('fp6WifiAuthField').text = '1234'", "el('fp6WifiAuthConfirm').clicked()", ""],
     delay=1200)
shot("08-share-qr-dark-hidden-wpa3", "NetworkDetails.qml", DETAILS,
     [open_share, "el('fp6WifiAuthField').text = '1234'", "el('fp6WifiAuthConfirm').clicked()", ""], delay=1200,
     theme="SuruDark", extra_env={"FP6_WIFIQR_FAKE_SECURITY": "sae", "FP6_WIFIQR_FAKE_HIDDEN": "1"})
shot("09-share-open-network", "NetworkDetails.qml", '{ networkName: "Cafe Guest", dbusPath: "/fake/2", lastUsed: "" }',
     [open_share, ""], delay=1200, extra_env={"FP6_WIFIQR_FAKE_SECURITY": "open"})
shot("10-details-enterprise", "NetworkDetails.qml", '{ networkName: "Corp", dbusPath: "/fake/3", lastUsed: "" }',
     extra_env={"FP6_WIFIQR_FAKE_SECURITY": "enterprise"})
shot("11-share-no-screen-lock", "NetworkDetails.qml", DETAILS, [open_share, ""], delay=1200,
     extra_env={"FP6_WIFIQR_FAKE_LOCK": "swipe"})

scan("20-scanner-not-wifi", payload="https://example.org/menu", delay=2500)
scan("21-scanner-join-sheet")
scan("22-scanner-join-hidden-wpa3-saved", payload="WIFI:T:SAE;S:Lab Net;P:longpassword;H:true;;",
     extra_env={"FP6_WIFIQR_FAKE_SAVED": "1"})
scan("23-scanner-connecting", step="connecting", delay=3200, extra_env={"FP6_WIFIQR_FAKE_JOIN_MS": "60000"})
scan("24-scanner-connected", step="connected", delay=6000)
scan("27-scanner-wrong-password", step="connecting", extra_env={"FP6_WIFIQR_FAKE_JOIN": "fail", "FP6_WIFIQR_FAKE_JOIN_MS": "500"})
scan("25-scanner-enterprise", payload="WIFI:T:WPA2-EAP;S:Corp;E:PEAP;PH2:MSCHAPV2;I:someone;P:abcdefgh;;")
scan("26-scanner-bad-password", payload="WIFI:T:WPA;S:Test\\;Net;P:short;;")

bad = [n for n, ok, b in results if not ok or b]
print(f"\n{len(results) - len(bad)}/{len(results)} shots clean; logs in {scratch}")
sys.exit(1 if bad else 0)
