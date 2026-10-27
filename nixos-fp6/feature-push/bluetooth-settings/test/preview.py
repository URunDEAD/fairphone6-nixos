#!/usr/bin/env python3
"""Laptop (x86) previews and functional checks of the FP6 Bluetooth settings.

  nix-build test -o /tmp/fp6-bt-env
  test/preview.py /tmp/fp6-bt-env OUTDIR [shot ...]

Runs everything on private D-Bus buses: python-dbusmock's BlueZ template as
the "system" bus (one adapter, synthetic devices with 02:00:00:... addresses
and made-up names), the patched ayatana-indicator-bluetooth on a private
session bus, and the real FP6 System Settings Bluetooth plugin in qmlscene
(offscreen, FP6 size 1116x2412, GRID_UNIT_PX=24). Besides screenshots it
checks the BlueZ properties the panel sets (Discoverable/Pairable and their
timeouts, Alias), the agent's refusal of unsolicited pairing requests, the
per-user store files, and dumps the indicator's phone menu as text.
"""
import json, os, shutil, subprocess, sys, tempfile, time

import dbus

env_bin = os.path.abspath(sys.argv[1]) + "/bin/fp6-bt-env"
out = os.path.abspath(sys.argv[2])
only = sys.argv[3:]
os.makedirs(out, exist_ok=True)
scratch = tempfile.mkdtemp(prefix="fp6-bt-preview-")
envsh = open(env_bin).read()


def var(n):
    return envsh.split(f"export {n}=")[1].split("\n")[0].strip()


QML_SRC, INDICATOR, PY = var("FP6_BT_QML"), var("FP6_BT_INDICATOR"), var("FP6_PYTHON")
DBUS_DAEMON, DBUS_CONF, QMLSCENE = var("FP6_DBUS_DAEMON"), var("FP6_DBUS_CONF"), var("FP6_QMLSCENE")

qmldir = f"{scratch}/bluetooth"
shutil.copytree(QML_SRC, qmldir)
subprocess.run(["chmod", "-R", "u+w", qmldir], check=True)

home = f"{scratch}/home"
procs = []
results = []


def bus():
    p = subprocess.Popen([DBUS_DAEMON, f"--config-file={DBUS_CONF}", "--nofork", "--print-address=1"],
                         stdout=subprocess.PIPE, text=True)
    procs.append(p)
    return p.stdout.readline().strip()


SYSTEM, SESSION = bus(), bus()
base_env = dict(os.environ)
for k in ["QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE"]:
    base_env.pop(k, None)
base_env.update({
    "HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "XDG_CACHE_HOME": f"{home}/.cache",
    "XDG_STATE_HOME": f"{home}/.local/state", "QT_QPA_PLATFORM": "offscreen", "GRID_UNIT_PX": "24",
    "LANG": "en_US.UTF-8", "DBUS_SYSTEM_BUS_ADDRESS": SYSTEM, "DBUS_SESSION_BUS_ADDRESS": SESSION,
    "XDG_CURRENT_DESKTOP": "Lomiri",
})
os.makedirs(f"{home}/.config/fp6-bluetooth", exist_ok=True)
os.makedirs(f"{home}/.local/state/fp6-bluetooth", exist_ok=True)

sysbus = dbus.bus.BusConnection(SYSTEM)

# ---- fake BlueZ ----------------------------------------------------------
procs.append(subprocess.Popen([env_bin, PY, "-m", "dbusmock", "--system", "--template", "bluez5"],
                              env=base_env, stdout=subprocess.DEVNULL, stderr=open(f"{scratch}/dbusmock.log", "w")))
for _ in range(100):
    if sysbus.name_has_owner("org.bluez"):
        break
    time.sleep(0.1)
mock = dbus.Interface(sysbus.get_object("org.bluez", "/org/bluez"), "org.bluez.Mock")
mock.AddAdapter("hci0", "Test Phone")
ADAPTER = "/org/bluez/hci0"


def dmock(path):
    return dbus.Interface(sysbus.get_object("org.bluez", path), "org.freedesktop.DBus.Mock")


def props(path, iface):
    return dbus.Interface(sysbus.get_object("org.bluez", path), "org.freedesktop.DBus.Properties").GetAll(iface)


A2DP, HFP, HID, OPP = ("0000110b-0000-1000-8000-00805f9b34fb", "0000111e-0000-1000-8000-00805f9b34fb",
                       "00001124-0000-1000-8000-00805f9b34fb", "00001105-0000-1000-8000-00805f9b34fb")
DEVICES = [
    # address, name, class, appearance, uuids, paired, connected, battery
    ("02:00:00:00:00:01", "BT-01", 0x240404, 0, [A2DP, HFP], True, True, 80),
    ("02:00:00:00:00:02", "Test Buds", 0, 0x0941, [A2DP, HFP], True, False, None),
    ("02:00:00:00:00:03", "Desk Keyboard", 0x002540, 0, [HID], True, False, None),
    ("02:00:00:00:00:04", "Living Room TV", 0x24043c, 0, [A2DP], False, False, None),
    ("02:00:00:00:00:05", "", 0x5a020c, 0, [OPP], False, False, None),
]


def key(addr):
    return addr.upper()


# user store: BT-01 is really a car speaker; buds last connected 2 days ago
open(f"{home}/.config/fp6-bluetooth/devices.ini", "w").write(
    f"[{key(DEVICES[0][0])}]\ntype=car\n")
open(f"{home}/.local/state/fp6-bluetooth/last-connected.ini", "w").write(
    f"[{key(DEVICES[1][0])}]\nlast-connected={int(time.time()) - 2 * 86400 - 600}\n"
    f"[{key(DEVICES[2][0])}]\nlast-connected={int(time.time()) - 3 * 3600}\n")

paths = {}
for addr, name, cls, app, uuids, paired, connected, battery in DEVICES:
    p = mock.AddDevice("hci0", addr, name)
    paths[addr] = p
    upd = {"Class": dbus.UInt32(cls), "Appearance": dbus.UInt16(app), "UUIDs": dbus.Array(uuids, signature="s"),
           "Icon": dbus.String({0x240404: "audio-headset", 0x002540: "input-keyboard"}.get(cls, "audio-card"))}
    if not name:
        upd.update({"Name": dbus.String(""), "Alias": dbus.String(addr.replace(":", "-"))})
    if paired:
        mock.PairDevice("hci0", addr)
        upd["Trusted"] = dbus.Boolean(True)
    if connected:
        mock.ConnectDevice("hci0", addr)
    # after PairDevice: the mock's pairing sets phone UUIDs
    dmock(p).UpdateProperties("org.bluez.Device1", upd)
    if battery is not None:
        dmock(p).AddProperties("org.bluez.Battery1", {"Percentage": dbus.Byte(battery)})

# ---- patched indicator ---------------------------------------------------
ind_log = open(f"{scratch}/indicator.log", "w")
procs.append(subprocess.Popen([env_bin, INDICATOR], env=base_env, stdout=ind_log, stderr=ind_log))
sesbus = dbus.bus.BusConnection(SESSION)
for _ in range(100):
    if sesbus.name_has_owner("org.ayatana.indicator.bluetooth"):
        break
    time.sleep(0.1)

MENU_DUMP = r'''
import sys
from gi.repository import Gio, GLib
conn = Gio.DBusConnection.new_for_address_sync(sys.argv[1],
    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None, None)
name, base = "org.ayatana.indicator.bluetooth", "/org/ayatana/indicator/bluetooth"
r = conn.call_sync(name, base + "/phone", "org.gtk.Menus", "Start", GLib.Variant("(au)", (list(range(40)),)),
                   None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
menus = {(g, m): items for g, m, items in r}
acts = conn.call_sync(name, base, "org.gtk.Actions", "DescribeAll", None, None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
def state(a):
    a = a.split(".", 1)[-1]
    st = acts.get(a, (None, None, []))[2]
    return st[0] if st else "-"
out = []
def walk(key, depth):
    for it in menus.get(key, []):
        if ":section" in it:
            walk(tuple(it[":section"]), depth); continue
        line = "  " * depth + "- " + it.get("label", "(no label)")
        if "x-ayatana-type" in it: line += "  [" + it["x-ayatana-type"].split(".")[-1] + "]"
        if "icon" in it:
            ic = it["icon"]
            line += "  icon=" + (ic[1][0] if isinstance(ic, tuple) and isinstance(ic[1], list) else str(ic))
        if "action" in it:
            st = state(it["action"])
            line += "  state=" + (str(st) if not isinstance(st, dict) else "{root}")
        if "x-ayatana-subtitle-action" in it:
            line += "  subtitle=" + str(state(it["x-ayatana-subtitle-action"]))
        out.append(line)
        if ":submenu" in it:
            walk(tuple(it[":submenu"]), depth + 1)
walk((0, 0), 0)
print("\n".join(out))
'''
open(f"{scratch}/menu_dump.py", "w").write(MENU_DUMP)


def dump_menu(name):
    if only and name not in only:
        return
    r = subprocess.run([env_bin, PY, f"{scratch}/menu_dump.py", SESSION], env=base_env, capture_output=True, text=True,
                       timeout=30)
    open(f"{out}/{name}.txt", "w").write(r.stdout)
    ok = "BT-01" in r.stdout and "Bluetooth settings" in r.stdout
    results.append((name, ok, [] if ok else [r.stderr[-500:]]))
    print(f"menu {name}: {'ok' if ok else 'BAD'}\n" + r.stdout)


HARNESS = """import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.SystemSettings.Bluetooth 1.0
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri-system-settings"
    // grabToImage() does not include the window clear colour
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPage: first
    }
    property var first: null
    property var backend: first ? first.fp6Backend : null
    Component.onCompleted: {
        var c = Qt.createComponent("%PAGE%");
        if (c.status !== Component.Ready) console.log("HARNESS: " + c.errorString());
        first = c.createObject(pageStack, {});
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
    function openDevice(addr) {
        backend.setSelectedDevice(addr)
        pageStack.addPageToNextColumn(first, Qt.resolvedUrl("DevicePage.qml"), {backend: backend, root: first})
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
}
"""

# The page keeps its backend private; expose it for the harness only.
pc = f"{qmldir}/PageComponent.qml"
s = open(pc).read()
s = s.replace('    objectName: "bluetoothPage"\n', '    objectName: "bluetoothPage"\n    property alias fp6Backend: backend\n', 1)
open(pc, "w").write(s)


def check_log(name, log, ok_extra=True, notes=()):
    bad = [l for l in log.splitlines() if any(k in l for k in ("Error", "error:", "HARNESS", "TypeError", "ReferenceError",
                                                             "is not defined", "Cannot assign", "not a type", "failed"))
           and "Binding loop" not in l and "ItemPage.qml:32" not in l]
    ok = os.path.exists(f"{out}/{name}.png") and ok_extra
    results.append((name, ok, bad + list(notes)))
    print(f"shot {name}: {'saved' if ok else 'MISSING/BAD'}" + ("" if not (bad or notes) else "\n  " + "\n  ".join((bad + list(notes))[:12])))


def write_settings(**kw):
    p = f"{home}/.config/fp6-bluetooth/settings.ini"
    if not kw:
        if os.path.exists(p):
            os.remove(p)
        return
    open(p, "w").write("[General]\n" + "".join(f"{k}={v}\n" for k, v in kw.items()))


def shot(name, steps=(), delay=900, theme="Ambiance", during=None, after=None, settings=None):
    if only and name not in only:
        return
    write_settings(**(settings or {}))
    qml = HARNESS.replace("%PAGE%", f"file://{qmldir}/PageComponent.qml").replace("%DELAY%", str(delay)) \
        .replace("%OUT%", f"{out}/{name}.png").replace("%STEPS%", ", ".join(f"function() {{ {s} }}" for s in steps))
    path = f"{qmldir}/harness-{name}.qml"
    open(path, "w").write(qml)
    os.makedirs(f"{home}/.config/lomiri-ui-toolkit", exist_ok=True)
    open(f"{home}/.config/lomiri-ui-toolkit/theme.ini", "w").write(f"[General]\ntheme=Lomiri.Components.Themes.{theme}\n")
    p = subprocess.Popen([env_bin, QMLSCENE, path], env=base_env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    notes = []
    ok_extra = True
    if during:
        ok_extra, notes = during(p)
    o, _ = p.communicate(timeout=120)
    log = "\n".join(l for l in o.splitlines() if "fontconfig" not in l.lower())
    open(f"{scratch}/{name}.log", "w").write(log)
    if after:
        ok2, n2 = after()
        ok_extra = ok_extra and ok2
        notes = list(notes) + list(n2)
    check_log(name, log, ok_extra, notes)


def adapter():
    return props(ADAPTER, "org.bluez.Adapter1")


def reset_adapter():
    dmock(ADAPTER).UpdateProperties("org.bluez.Adapter1", {
        "Discoverable": dbus.Boolean(False), "Pairable": dbus.Boolean(False),
        "DiscoverableTimeout": dbus.UInt32(180), "PairableTimeout": dbus.UInt32(0), "Alias": dbus.String("Test Phone")})


def lss_agent_owner():
    # the panel's agent: try every unique name that is not the mock
    mock_owner = sysbus.get_name_owner("org.bluez")
    names = [n for n in dbus.Interface(sysbus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"),
                                       "org.freedesktop.DBus").ListNames() if n.startswith(":") and n != mock_owner]
    for n in names:
        try:
            dbus.Interface(sysbus.get_object(n, "/com/lomiri/SettingsBluetoothAgent/adapteragent", introspect=False),
                           "org.freedesktop.DBus.Introspectable").Introspect(timeout=2)
            return n
        except dbus.DBusException:
            continue
    return None


def expect_adapter(want, label):
    def check(p=None):
        time.sleep(3.0)
        a = adapter()
        got = {k: (bool(a[k]) if isinstance(a[k], dbus.Boolean) else (int(a[k]) if isinstance(a[k], (dbus.UInt32,)) else str(a[k])))
               for k in want}
        ok = got == want
        return ok, [f"{label}: adapter {json.dumps(got)}" + ("" if ok else f" != {json.dumps(want)}")]
    return check


def after_close():
    time.sleep(1.0)
    a = adapter()
    ok = not bool(a["Discoverable"]) and not bool(a["Pairable"])
    return ok, [f"after closing: Discoverable={bool(a['Discoverable'])} Pairable={bool(a['Pairable'])}"]


# ---- shots --------------------------------------------------------------------
dump_menu("00-indicator-phone-menu")

reset_adapter()
shot("01-bluetooth-page", delay=1200, steps=["", ""], after=after_close,
     during=expect_adapter({"Discoverable": True, "Pairable": True, "DiscoverableTimeout": 180, "PairableTimeout": 180},
                           "page open, defaults"))
shot("01b-bluetooth-page-dark", delay=1200, steps=["", ""], theme="SuruDark")


reset_adapter()
shot("02-pairing-off-hidden", delay=1200, steps=["", ""], settings={"visible": "false", "allow-pairing": "false"},
     during=expect_adapter({"Discoverable": False, "Pairable": False}, "visible+pairing off"))

reset_adapter()
shot("03-visible-5-min", delay=900, steps=["el('fp6VisibilityTimeout').expanded = true", "", ""],
     settings={"visibility-timeout": "300"},
     during=expect_adapter({"Discoverable": True, "DiscoverableTimeout": 300}, "visible for 5 minutes"))


def inactive_check(p):
    # Qt.application.state cannot be changed in qmlscene; the backend's
    # active flag is what the page binds to.
    return expect_adapter({"Discoverable": False, "Pairable": False}, "backend.active=false (app in background)")(p)


reset_adapter()
shot("04-app-in-background", delay=900, steps=["", "", "mv.first.fp6Backend.active = false"],
     during=lambda p: (time.sleep(2.0), inactive_check(p))[1])


def unsolicited(p, expect_reject):
    time.sleep(3.0)
    owner = lss_agent_owner()
    if not owner:
        return False, ["agent not found on the bus"]
    agent = dbus.Interface(sysbus.get_object(owner, "/com/lomiri/SettingsBluetoothAgent/adapteragent", introspect=False),
                           "org.bluez.Agent1")
    try:
        agent.RequestConfirmation(dbus.ObjectPath(paths["02:00:00:00:00:04"]), dbus.UInt32(123456), timeout=2.5)
        res = "accepted"
    except dbus.DBusException as e:
        res = e.get_dbus_name()
    if expect_reject:
        ok = res == "org.bluez.Error.Rejected"
    else:
        ok = res == "org.freedesktop.DBus.Error.NoReply"  # dialog open, waiting for the user
    return ok, [f"unsolicited RequestConfirmation -> {res}"]


reset_adapter()
shot("05-incoming-pairing-dialog", delay=1300, steps=["", "", "", "", ""],
     during=lambda p: unsolicited(p, expect_reject=False))
reset_adapter()
shot("06-incoming-pairing-refused", delay=1200, steps=["", "", "", ""], settings={"allow-pairing": "false"},
     during=lambda p: unsolicited(p, expect_reject=True))

reset_adapter()
shot("07-rename-phone-dialog", delay=900, steps=["", "", "el('fp6PhoneName').clicked()", "el('fp6RenameField').text = 'Test FP6 renamed'"])


def check_alias():
    a = adapter()
    ok = str(a["Alias"]) == "Test FP6 renamed"
    return ok, [f"adapter Alias after save: {a['Alias']}"]


reset_adapter()
shot("07b-rename-phone-saved", delay=900, steps=["", "", "el('fp6PhoneName').clicked()", "el('fp6RenameField').text = 'Test FP6 renamed'",
                                                  "el('fp6RenameSave').clicked()", ""],
     after=check_alias)

reset_adapter()
shot("08-device-page-car", delay=900, steps=["", "", "mv.openDevice('02:00:00:00:00:01')", ""])
shot("09-device-page-address-shown", delay=900, steps=["", "", "mv.openDevice('02:00:00:00:00:02')", "el('fp6DeviceAddress').clicked()", ""])
shot("10-device-type-page", delay=900, steps=["", "", "mv.openDevice('02:00:00:00:00:03')", "el('fp6DeviceType').clicked()", ""])


def check_kind():
    path = f"{home}/.config/fp6-bluetooth/devices.ini"
    txt = open(path).read()
    k = "[" + key("02:00:00:00:00:03") + "]"
    ok = k in txt and "type=gamepad" in txt.split(k, 1)[1].split("[", 1)[0]
    mode = oct(os.stat(path).st_mode & 0o777)
    dmode = oct(os.stat(os.path.dirname(path)).st_mode & 0o777)
    ok = ok and mode == "0o600" and dmode == "0o700"
    return ok, [f"devices.ini (mode {mode}, dir {dmode}) after choosing Game controller:\n    " + txt.replace("\n", "\n    ")]


shot("11-device-type-chosen", delay=900, steps=["", "", "mv.openDevice('02:00:00:00:00:03')", "el('fp6DeviceType').clicked()",
                                                 "el('fp6Kind_gamepad').clicked()", ""], after=check_kind)
time.sleep(1.5)
dump_menu("12-indicator-after-type-change")


def check_dev_alias():
    d = props(paths["02:00:00:00:00:02"], "org.bluez.Device1")
    txt = open(f"{home}/.config/fp6-bluetooth/devices.ini").read()
    k = "[" + key("02:00:00:00:00:02") + "]"
    ok = str(d["Alias"]) == "Test Buds (renamed)" and k in txt and \
        "name=Test Buds (renamed)" in txt.split(k, 1)[1].split("[", 1)[0]
    return ok, [f"Device1.Alias after rename: {d['Alias']}; devices.ini name= written: {ok}"]


shot("13-device-rename", delay=900, steps=["", "", "mv.openDevice('02:00:00:00:00:02')", "el('fp6DeviceName').clicked()",
                                            "el('fp6RenameField').text = 'Test Buds (renamed)'", "el('fp6RenameSave').clicked()", ""],
     after=check_dev_alias)
time.sleep(1.5)
dump_menu("16-indicator-after-rename")
shot("14-forget-confirm", delay=900, steps=["", "", "mv.openDevice('02:00:00:00:00:03')", "el('fp6DeviceForget').clicked()", ""])
shot("15-device-page-dark", delay=900, theme="SuruDark", steps=["", "", "mv.openDevice('02:00:00:00:00:01')", ""])

for p in procs:
    p.terminate()
bad = [n for n, ok, b in results if not ok]
print(f"\n{len(results) - len(bad)}/{len(results)} checks ok; logs in {scratch}")
sys.exit(1 if bad else 0)
