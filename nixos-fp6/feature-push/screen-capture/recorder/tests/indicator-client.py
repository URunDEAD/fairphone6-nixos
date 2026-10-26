#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Check fp6-screenrecord-indicator the way Lomiri reads it (GDBusMenuModel +
GDBusActionGroup), against a fake shell (com.fp6.ScreenRecorder). Run inside
dbus-run-session with the indicator started; prints PASS/FAIL lines."""
import sys
import time

from gi.repository import Gio, GLib

bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
failures = 0
requests = []


def check(cond, what):
    global failures
    print(("PASS " if cond else "FAIL ") + what, flush=True)
    failures += 0 if cond else 1


def spin(seconds=0.5):
    ctx = GLib.MainContext.default()
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        ctx.iteration(False)
        time.sleep(0.01)


# fake shell
SHELL_XML = """<node><interface name="com.fp6.ScreenRecorder">
<method name="RequestStart"/><method name="Stop"/>
<method name="IsRecording"><arg type="b" direction="out"/></method>
<signal name="RecordingChanged"><arg type="b"/></signal></interface></node>"""
recording = [False]


def shell_call(conn, sender, path, iface, method, params, inv):
    requests.append(method)
    if method == "IsRecording":
        inv.return_value(GLib.Variant("(b)", (recording[0],)))
    else:
        inv.return_value(None)


node = Gio.DBusNodeInfo.new_for_xml(SHELL_XML)
bus.register_object("/com/fp6/ScreenRecorder", node.interfaces[0], shell_call, None, None)
Gio.bus_own_name_on_connection(bus, "com.fp6.ScreenRecorder", Gio.BusNameOwnerFlags.NONE, None, None)
spin(1.0)

name = "com.lomiri.indicator.screenrecord"
menu = Gio.DBusMenuModel.get(bus, name, "/com/lomiri/indicator/screenrecord/phone")
actions = Gio.DBusActionGroup.get(bus, name, "/com/lomiri/indicator/screenrecord")
menu.get_n_items()
actions.list_actions()
spin(1.0)
check(menu.get_n_items() == 1, f"one root item ({menu.get_n_items()})")
root_action = menu.get_item_attribute_value(0, "action", None)
check(root_action and root_action.get_string() == "indicator.root", "root item -> indicator.root")
sub = menu.get_item_link(0, "submenu")
sub.get_n_items(); spin(0.5)
check(sub is not None and sub.get_n_items() == 1, "submenu with one item")
label = sub.get_item_attribute_value(0, "label", None)
typ = sub.get_item_attribute_value(0, "x-ayatana-type", None)
check(label and label.get_string() == "Record screen", "switch label")
check(typ and typ.get_string() == "org.ayatana.indicator.switch", "switch type")
check(set(actions.list_actions()) == {"root", "recording"}, f"actions {actions.list_actions()}")
st = actions.get_action_state("root")
check(st is not None and st.unpack().get("visible") is False and "icon" in st.unpack(), f"root state {st}")
icon = Gio.Icon.deserialize(st.lookup_value("icon", None))
check(isinstance(icon, Gio.ThemedIcon) and "media-record" in icon.get_names(), "root icon deserializes")
check(actions.get_action_state("recording").get_boolean() is False, "recording false at start")

actions.activate_action("recording", None); spin(0.5)
check(requests.count("RequestStart") == 1, f"switch on -> RequestStart {requests}")
check(actions.get_action_state("recording").get_boolean() is False, "still off until the shell says so")

recording[0] = True
bus.emit_signal(None, "/com/fp6/ScreenRecorder", "com.fp6.ScreenRecorder", "RecordingChanged", GLib.Variant("(b)", (True,)))
spin(0.5)
check(actions.get_action_state("recording").get_boolean() is True, "RecordingChanged(true) -> switch on")
actions.change_action_state("recording", GLib.Variant("b", False)); spin(0.5)
check(requests.count("Stop") == 1, f"switch off -> Stop {requests}")
sys.exit(1 if failures else 0)
