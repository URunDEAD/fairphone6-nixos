#!/usr/bin/env python3
"""Mock D-Bus services for the lock-sounds QML test (one private bus that
is both the session and the "system" bus):

  * org.ayatana.indicator.sound: org.gtk.Actions at
    /org/ayatana/indicator/sound with "silent-mode" (b, settable) and the
    test control action "fp6-test-display" (s "STATUS:REASON") which makes
    the mock repowerd emit DisplayPowerStateChange;
  * com.canonical.Unity.Screen: /com/canonical/Unity/Screen, the signal the
    Lomiri Powerd plugin listens to (Powerd.status / statusChanged(reason)).
"""
import sys

from gi.repository import Gio, GLib

SCREEN_XML = """
<node>
  <interface name="com.canonical.Unity.Screen">
    <method name="setScreenPowerMode"><arg type="s" direction="in"/><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="setInactivityTimeouts"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="setUserBrightness"><arg type="i" direction="in"/></method>
    <method name="userAutobrightnessEnable"><arg type="b" direction="in"/></method>
    <method name="setDoubleTapToWakeEnabled"><arg type="b" direction="in"/></method>
    <signal name="DisplayPowerStateChange"><arg type="i"/><arg type="i"/></signal>
  </interface>
</node>
"""

conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
group = Gio.SimpleActionGroup()

silent = Gio.SimpleAction.new_stateful("silent-mode", None, GLib.Variant("b", False))
silent.connect("change-state", lambda a, v: a.set_state(v))
silent.connect("activate", lambda a, p: a.set_state(GLib.Variant("b", not a.get_state().unpack())))
group.add_action(silent)


def display(_action, param):
    status, reason = (int(x) for x in param.unpack().split(":"))
    conn.emit_signal(None, "/com/canonical/Unity/Screen", "com.canonical.Unity.Screen",
                     "DisplayPowerStateChange", GLib.Variant("(ii)", (status, reason)))


ctl = Gio.SimpleAction.new("fp6-test-display", GLib.VariantType.new("s"))
ctl.connect("activate", display)
group.add_action(ctl)

conn.export_action_group("/org/ayatana/indicator/sound", group)


def screen_call(_c, _sender, _path, _iface, method, _params, invocation):
    invocation.return_value(GLib.Variant("(b)", (True,)) if method == "setScreenPowerMode" else None)


node = Gio.DBusNodeInfo.new_for_xml(SCREEN_XML)
conn.register_object("/com/canonical/Unity/Screen", node.interfaces[0], screen_call, None, None)

for name in ("org.ayatana.indicator.sound", "com.canonical.Unity.Screen"):
    r = conn.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
                       GLib.Variant("(su)", (name, 4)), GLib.VariantType.new("(u)"), 0, -1, None)
    if r.unpack()[0] != 1:
        sys.exit(f"could not own {name}")
print("mock-bus ready", flush=True)
GLib.MainLoop().run()
