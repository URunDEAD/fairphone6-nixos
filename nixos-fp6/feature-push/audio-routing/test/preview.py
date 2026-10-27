#!/usr/bin/env python3
"""Laptop (x86) previews of the audio routing UI, offscreen.

  nix-build test/qt-env.nix -o /tmp/fp6-audio-qt
  test/preview.py /tmp/fp6-audio-qt OUTDIR

A mock org.ayatana.indicator.sound (mock-indicator.py: same actions and
menu layout as the patched service, FP6 device lists) runs on a private
session bus. Rendered at the FP6's 1116x2412 (GRID_UNIT_PX=24):
  * Lomiri's sound indicator menu, with Lomiri's own
    IndicatorMenuItemFactory.qml over QMenuModel (AyatanaMenuModel);
  * Settings > Sound, the stock PageComponent.qml with
    lss-sound-devices.patch applied.
Each scenario also taps a device and checks that the tick moves.
"""
import os, shutil, subprocess, sys, tempfile, time

qt = os.path.abspath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
here = os.path.dirname(os.path.abspath(__file__))
os.makedirs(out, exist_ok=True)
scratch = tempfile.mkdtemp(prefix="fp6-audio-preview-")
qmlscene = f"{qt}/bin/fp6-qmlscene"
envsh = open(qmlscene).read()


def envvar(name):
    return envsh.split(f"{name}=")[1].split("\n")[0].strip()


python, typelibs, lss, lomiri = envvar("FP6_PYTHON"), envvar("FP6_GI_TYPELIB_PATH"), envvar("FP6_LSS"), envvar("FP6_LOMIRI")

# Patched Settings > Sound page next to its stock siblings
page_dir = f"{scratch}/sound"
shutil.copytree(f"{lss}/share/lomiri-system-settings/qml-plugins/sound", page_dir)
os.chmod(page_dir, 0o755)
for f in os.listdir(page_dir):
    os.chmod(f"{page_dir}/{f}", 0o644)
subprocess.run(["patch", "-s", "-p3", "-d", page_dir, "-i", f"{here}/../lss-sound-devices.patch"], check=True)
shutil.copy(f"{here}/../AppAudioPage.qml", f"{page_dir}/AppAudioPage.qml")

env = dict(os.environ)
for k in ["QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE", "DBUS_SESSION_BUS_ADDRESS"]:
    env.pop(k, None)
home = f"{scratch}/home"
os.makedirs(f"{home}/.config/lomiri-ui-toolkit", exist_ok=True)
bus = f"fp6audio-preview-{os.getpid()}"
env.update({"HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "DBUS_SESSION_BUS_ADDRESS": f"unix:abstract={bus}",
            "LANG": "en_US.UTF-8", "GI_TYPELIB_PATH": typelibs})
with open(f"{scratch}/bus.conf", "w") as f:
    f.write(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:abstract={bus}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>""")
procs = [subprocess.Popen(["dbus-daemon", "--config-file", f"{scratch}/bus.conf", "--nofork"], env=env)]
time.sleep(0.5)

HARNESS_SETTINGS = """import QtQuick 2.12
import Lomiri.Components 1.3
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri-system-settings"
    property bool showAllUI: false
    property bool isSnap: false
    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("%PAGE%")
    }
    %COMMON%
}
"""

HARNESS_MENU = """import QtQuick 2.12
import Lomiri.Components 1.3
import QMenuModel 1.0
import "file://%LOMIRI%/share/lomiri/Panel/Indicators"
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri"
    theme.name: "Lomiri.Components.Themes.SuruDark"
    backgroundColor: "#111111"
    AyatanaMenuModel {
        id: rootModel
        busName: "org.ayatana.indicator.sound"
        actions: { "indicator": "/org/ayatana/indicator/sound" }
        menuObjectPath: "/org/ayatana/indicator/sound/phone"
    }
    property var menuModel: null
    Timer { interval: 300; running: true; onTriggered: { mv.menuModel = rootModel.submenu(0) } }
    Rectangle { anchors.fill: parent; color: "#111111" }
    Rectangle {
        id: header
        anchors { left: parent.left; right: parent.right; top: parent.top }
        height: units.gu(6)
        color: "#1d1d1d"
        Label { anchors.centerIn: parent; text: "Sound"; color: "white"; textSize: Label.Large }
    }
    IndicatorMenuItemFactory { id: factory; indicator: "ayatana-indicator-sound"; menuModel: mv.menuModel }
    ListView {
        id: listView
        anchors { left: parent.left; right: parent.right; top: header.bottom; bottom: parent.bottom }
        model: mv.menuModel
        delegate: Loader {
            objectName: "menuItem" + index
            width: ListView.view.width
            property int modelIndex: index
            sourceComponent: factory.load(model)
            onLoaded: {
                if (item.hasOwnProperty("menuData")) item.menuData = Qt.binding(function() { return model; })
                if (item.hasOwnProperty("menuIndex")) item.menuIndex = Qt.binding(function() { return modelIndex; })
                item.objectName = model.label || ("item" + index)
            }
        }
    }
    %COMMON%
}
"""

COMMON = """
    function find(item, name) {
        if (!item) return null
        if (item.objectName === name) return item
        var ch = item.children || []
        for (var i = 0; i < ch.length; i++) { var r = find(ch[i], name); if (r) return r }
        if (item.contentItem && item.contentItem !== item) { var c = find(item.contentItem, name); if (c) return c }
        return null
    }
    function el(name) { var r = find(mv, name); if (!r) console.log("HARNESS: no " + name); return r }
    function ticks() {
        var n = 0
        function walk(it) { if (!it) return; if (it.objectName === "activeTick" && it.visible) n++;
            var ch = it.children || []; for (var i = 0; i < ch.length; i++) walk(ch[i]);
            if (it.contentItem && it.contentItem !== it && (it.children || []).indexOf(it.contentItem) < 0) walk(it.contentItem) }
        walk(mv); return n
    }
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
"""

failures = []


def shot(name, kind, steps=(), delay=1500, theme="Ambiance"):
    if kind == "settings":
        qml = HARNESS_SETTINGS.replace("%PAGE%", f"file://{page_dir}/PageComponent.qml")
    else:
        qml = HARNESS_MENU.replace("%LOMIRI%", lomiri)
    qml = qml.replace("%COMMON%", COMMON).replace("%DELAY%", str(delay)).replace("%OUT%", f"{out}/{name}.png") \
        .replace("%STEPS%", ", ".join(f"function() {{ {s} }}" for s in steps))
    path = f"{scratch}/harness-{name}.qml"
    open(path, "w").write(qml)
    open(f"{home}/.config/lomiri-ui-toolkit/theme.ini", "w").write(f"[General]\ntheme=Lomiri.Components.Themes.{theme}\n")
    e = dict(env, QT_QPA_PLATFORM="offscreen", GRID_UNIT_PX="24")
    if os.path.exists(f"{out}/{name}.png"):
        os.unlink(f"{out}/{name}.png")
    r = subprocess.run([qmlscene, path], env=e, capture_output=True, text=True, timeout=90)
    log = "\n".join(l for l in (r.stdout + r.stderr).splitlines() if "fontconfig" not in l.lower())
    open(f"{scratch}/{name}.log", "w").write(log)
    bad = [l for l in log.splitlines() if ("rror" in l or "HARNESS" in l or "is not defined" in l or "Cannot" in l
                                            or "CHECK" in l) and "Binding loop" not in l]
    ok = os.path.exists(f"{out}/{name}.png")
    print(f"shot {name}: {'saved' if ok else 'MISSING'}" + ("" if not bad else "\n  " + "\n  ".join(bad[:12])))
    for l in log.splitlines():
        if "CHECK FAILED" in l:
            failures.append(f"{name}: {l}")
    if not ok:
        failures.append(f"{name}: no image")


def scenario(name):
    m = subprocess.Popen([python, f"{here}/mock-indicator.py", name], env=env)
    time.sleep(1.0)
    return m


try:
    m = scenario("music")
    shot("01-indicator-music", "menu", delay=1200, theme="SuruDark")
    # Tap "Phone speaker": the radio state follows the service
    shot("02-indicator-music-tap-speaker", "menu",
         ["var it = el('Phone speaker'); it.clicked()", "",
          "var it = el('Phone speaker'); if (!it || !it.checked) console.log('CHECK FAILED: Phone speaker not ticked after tap'); var b = el('Test Speaker'); if (b && b.checked) console.log('CHECK FAILED: Test Speaker still ticked')"],
         delay=900, theme="SuruDark")
    m.terminate(); m.wait()

    m = scenario("music")
    shot("03-settings-sound-music", "settings", ["var o = el('outputDevices'), i = el('inputDevices'); var no = 0, ni = 0; for (var k = 0; k < o.count; k++) if (o.itemAt(k).item.device.active) no++; for (k = 0; k < i.count; k++) if (i.itemAt(k).item.device.active) ni++; if (no !== 1 || ni !== 1 || o.count !== 3 || i.count !== 4) console.log('CHECK FAILED: outputs ' + o.count + '/' + no + ' inputs ' + i.count + '/' + ni)"], delay=1500)
    shot("04-settings-sound-tap-usb", "settings",
         ["var r = el('outputDevices'); r.itemAt(1).item.clicked()", "",
          "var r = el('outputDevices'); if (!r.itemAt(1).item.device.active) console.log('CHECK FAILED: USB not active after tap'); if (r.itemAt(2).item.device.active) console.log('CHECK FAILED: Test Speaker still active')"],
         delay=900)
    shot("05-settings-sound-dark", "settings", delay=1500, theme="SuruDark")
    shot("08-settings-sound-bottom", "settings", ["el('soundPage').flickable.contentY = units.gu(40)"], delay=1200)
    shot("09-app-audio", "settings", ["el('appAudio').clicked()", ""], delay=1200)
    shot("10-app-audio-firefox", "settings",
         ["el('appAudio').clicked()", "el('app_firefox').clicked()",
          "el('outputChoice3').clicked()", "",
          "var a = el('appAudioPage').appByKey('firefox'); if (a.output !== 'bluez_card.XX_XX_XX_XX_XX_XX|headset-output') console.log('CHECK FAILED: firefox output ' + a.output)"],
         delay=1000)
    shot("11-auto-switch-off", "settings",
         ["el('switchToNewDevices').trigger()", "", "el('soundPage').flickable.contentY = units.gu(40)",
          "if (el('switchToNewDevices').checked) console.log('CHECK FAILED: switch still on')"], delay=1000)
    m.terminate(); m.wait()

    m = scenario("call")
    shot("06-indicator-call", "menu", delay=1200, theme="SuruDark")
    shot("07-settings-sound-call", "settings", delay=1500)
    m.terminate(); m.wait()
finally:
    for p in procs:
        p.terminate()
    print("logs:", scratch)

if failures:
    print("FAILURES:\n  " + "\n  ".join(failures))
    sys.exit(1)
print("all checks passed")
