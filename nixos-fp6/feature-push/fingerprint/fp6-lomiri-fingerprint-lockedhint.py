#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Mirror the Lomiri lock screen into logind's LockedHint.

fingerprintd's standby unlock (upstream "--standby-unlock=on") arms a verify
only for the seat session that logind reports as LockedHint=yes and Active,
and answers a match with org.freedesktop.login1.Session.Unlock. Lomiri honours
that Unlock signal (DBusLomiriSessionService::doUnlock -> Greeter.onUnlocked)
but never sets LockedHint itself, so on Lomiri the daemon never arms.

This user service sets LockedHint = (greeter IsActive) AND (display on):
  * com.lomiri.LomiriGreeter.IsActive on the session bus (/com/lomiri/LomiriGreeter)
  * com.canonical.Unity.Screen.DisplayPowerStateChange on the system bus
    (fp6-lomiri-powerd emulates screen-off with DCS brightness 0, leaving
    bl_power at 0, so fingerprintd's own display check cannot see it).

It only calls SetLockedHint on the caller's own display session (logind
permits that for the session owner). It never calls Unlock, never talks to
fingerprintd and never reads any biometric state. On exit it clears the hint.
"""
import os
import signal
import sys

import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

LOGIN1 = "org.freedesktop.login1"
SHELL = "com.lomiri.Shell"
GREETER_PATH = "/com/lomiri/LomiriGreeter"
GREETER_IFACE = "com.lomiri.LomiriGreeter"
SCREEN_IFACE = "com.canonical.Unity.Screen"
PROPS = "org.freedesktop.DBus.Properties"


def log(*args):
    print("fp6-lockedhint:", *args, flush=True)


class Bridge:
    def __init__(self, system, session):
        self.system, self.session = system, session
        self.greeter_active = False
        self.display_on = True   # no getter; fp6-lomiri-powerd starts with the display on
        self.applied = None
        session.add_signal_receiver(self.on_props, "PropertiesChanged", PROPS,
                                    path=GREETER_PATH, arg0=GREETER_IFACE)
        session.add_signal_receiver(self.on_owner, "NameOwnerChanged", "org.freedesktop.DBus",
                                    arg0=SHELL)
        system.add_signal_receiver(self.on_display, "DisplayPowerStateChange", SCREEN_IFACE)
        self.refresh_greeter()

    def session_path(self):
        manager = dbus.Interface(self.system.get_object(LOGIN1, "/org/freedesktop/login1"),
                                 LOGIN1 + ".Manager")
        user = self.system.get_object(LOGIN1, manager.GetUser(dbus.UInt32(os.getuid())))
        _sid, path = user.Get(LOGIN1 + ".User", "Display", dbus_interface=PROPS)
        if path == "/":
            raise RuntimeError("no display session for this user")
        return path

    def apply(self, force=False):
        want = bool(self.greeter_active and self.display_on)
        if want == self.applied and not force:
            return
        try:
            session = self.system.get_object(LOGIN1, self.session_path())
            session.SetLockedHint(dbus.Boolean(want), dbus_interface=LOGIN1 + ".Session")
            self.applied = want
            log("LockedHint", "yes" if want else "no",
                "(greeter", "active" if self.greeter_active else "inactive",
                "display", "on)" if self.display_on else "off)")
        except Exception as error:   # logind or the session may be going away
            log("SetLockedHint failed:", error)

    def refresh_greeter(self):
        try:
            obj = self.session.get_object(SHELL, GREETER_PATH)
            self.greeter_active = bool(obj.Get(GREETER_IFACE, "IsActive", dbus_interface=PROPS))
        except Exception as error:
            log("greeter not reachable yet:", error.__class__.__name__)
            self.greeter_active = False
        self.apply()
        return False

    def on_props(self, _iface, changed, invalidated):
        if "IsActive" in changed:
            self.greeter_active = bool(changed["IsActive"])
            self.apply()
        elif "IsActive" in invalidated:
            self.refresh_greeter()

    def on_owner(self, _name, _old, new):
        if new:
            GLib.timeout_add_seconds(1, self.refresh_greeter)
        else:
            self.greeter_active = False
            self.apply()

    def on_display(self, status, _reason):
        self.display_on = int(status) != 0
        self.apply()

    def clear(self):
        self.greeter_active = False
        self.apply(force=True)


def main():
    DBusGMainLoop(set_as_default=True)
    bridge = Bridge(dbus.SystemBus(), dbus.SessionBus())
    loop = GLib.MainLoop()

    def stop(*_):
        bridge.clear()
        loop.quit()
        return False

    for sig in (signal.SIGTERM, signal.SIGINT):
        GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, sig, stop)
    # Re-read the greeter periodically in case a signal was missed at startup.
    GLib.timeout_add_seconds(30, lambda: (bridge.refresh_greeter(), True)[1])
    loop.run()
    return 0


if __name__ == "__main__":
    sys.exit(main())
