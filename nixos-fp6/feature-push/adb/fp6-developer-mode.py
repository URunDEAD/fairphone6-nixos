#!/usr/bin/env python3
"""FP6 Developer mode backend for lomiri-system-settings (About > Developer mode).

lomiri-system-settings 1.3.x drives Developer mode through the usb_moded
D-Bus API (plugins/usb/usb-config-helper.cpp, Lomiri.SystemSettings.Usb):
  get_modes()  -> contains "_adb"  => the About page shows "Developer mode"
  get_config() -> "<mode>[_adb]"   => switch state
  set_config(s), set_mode(s)       => "_adb" suffix turns ADB on/off
  signal sig_usb_target_state_ind(s)
This service implements only that subset on the system bus as
com.meego.usb_moded. The USB function itself never changes: NCM networking
stays in the gadget and only ADB is added/removed (fp6-adbd.service).

Setters need polkit action org.fairphone6.developer-mode (granted to the
phone user by a polkit rule in adb-fp6.nix; D-Bus policy also limits the
setters to that user and root). The choice persists in STATE_FILE across reboots.
"""
import os
import subprocess
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

BUS_NAME = "com.meego.usb_moded"
OBJECT_PATH = "/com/meego/usb_moded"
INTERFACE = "com.meego.usb_moded"
# Overridable only for tests (an already registered action with the same defaults).
POLKIT_ACTION = os.environ.get("FP6_DEVMODE_POLKIT_ACTION", "org.fairphone6.developer-mode")
STATE_FILE = os.environ.get("FP6_DEVMODE_STATE", "/var/lib/fp6-adb/developer-mode")
ADBD_UNIT = os.environ.get("FP6_DEVMODE_UNIT", "fp6-adbd.service")
SYSTEMCTL = os.environ.get("FP6_SYSTEMCTL", "systemctl")
# Test hook: on a private bus the caller's bus name means nothing to polkit,
# so identify it by process instead (pid + start time + uid).
POLKIT_BY_PROCESS = os.environ.get("FP6_DEVMODE_POLKIT_SUBJECT") == "process"
IDLE_EXIT_SECONDS = int(os.environ.get("FP6_DEVMODE_IDLE_EXIT", "300"))
BASE_MODE = "rndis"  # NCM networking; the only mode this port offers
KNOWN_MODES = ("charging_only", "mtp", "rndis")


def log(msg):
    print(f"fp6-developer-mode: {msg}", flush=True)


def adb_enabled():
    return os.path.exists(STATE_FILE)


def config_string():
    return BASE_MODE + ("_adb" if adb_enabled() else "")


class UsbModed(dbus.service.Object):
    def __init__(self, bus, polkit_bus):
        super().__init__(bus, OBJECT_PATH)
        self.bus = bus
        self.polkit_bus = polkit_bus
        self.idle_source = None
        self.touch()

    # -- housekeeping -------------------------------------------------------
    def touch(self):
        if IDLE_EXIT_SECONDS <= 0:
            return
        if self.idle_source:
            GLib.source_remove(self.idle_source)
        self.idle_source = GLib.timeout_add_seconds(IDLE_EXIT_SECONDS, self.idle_exit)

    def idle_exit(self):
        log("idle, exiting (D-Bus activation restarts it on demand)")
        loop.quit()
        return False

    def authorize(self, sender):
        if POLKIT_BY_PROCESS:
            dbus_iface = dbus.Interface(
                self.bus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"),
                "org.freedesktop.DBus")
            pid = int(dbus_iface.GetConnectionUnixProcessID(sender))
            uid = int(dbus_iface.GetConnectionUnixUser(sender))
            with open(f"/proc/{pid}/stat") as f:
                start_time = int(f.read().rsplit(")", 1)[1].split()[19])
            subject = ("unix-process", {
                "pid": dbus.UInt32(pid), "start-time": dbus.UInt64(start_time),
                "uid": dbus.Int32(uid)})
        else:
            subject = ("system-bus-name", {"name": dbus.String(sender)})
        authority = dbus.Interface(
            self.polkit_bus.get_object("org.freedesktop.PolicyKit1",
                                       "/org/freedesktop/PolicyKit1/Authority"),
            "org.freedesktop.PolicyKit1.Authority")
        # flags 0: never interactive; the rule answers YES for the phone user.
        ok, _challenge, _details = authority.CheckAuthorization(
            dbus.Struct(subject, signature="sa{sv}"), POLKIT_ACTION,
            dbus.Dictionary({}, signature="ss"), dbus.UInt32(0), "", timeout=30)
        if not ok:
            log(f"denied for {sender}")
            raise dbus.exceptions.DBusException(
                "Developer mode can only be changed from the active phone session",
                name="org.freedesktop.DBus.Error.AccessDenied")

    def apply(self, requested, sender):
        requested = str(requested)
        mode = requested[:-4] if requested.endswith("_adb") else requested
        if mode not in KNOWN_MODES:
            raise dbus.exceptions.DBusException(
                f"unknown USB mode {requested!r}", name="com.meego.usb_moded.Error.UnknownMode")
        want = requested.endswith("_adb")
        if want == adb_enabled():
            return config_string()
        self.authorize(sender)
        if want:
            os.makedirs(os.path.dirname(STATE_FILE), mode=0o755, exist_ok=True)
            with open(STATE_FILE, "w") as f:
                f.write("enabled by the Developer mode switch\n")
            action = "start"
        else:
            os.unlink(STATE_FILE)
            action = "stop"
        # --no-block: the D-Bus reply must not wait for USB re-enumeration.
        subprocess.run([SYSTEMCTL, "--no-block", action, ADBD_UNIT], check=False)
        log(f"Developer mode {'on' if want else 'off'} ({action} {ADBD_UNIT}, by {sender})")
        state = config_string()
        self.sig_usb_target_state_ind(state)
        self.sig_usb_current_state_ind(state)
        return state

    # -- usb_moded subset ---------------------------------------------------
    @dbus.service.method(INTERFACE, in_signature="", out_signature="s")
    def get_modes(self):
        self.touch()
        return ",".join(m + s for m in (BASE_MODE,) for s in ("", "_adb"))

    @dbus.service.method(INTERFACE, in_signature="", out_signature="s")
    def get_available_modes(self):
        return self.get_modes()

    @dbus.service.method(INTERFACE, in_signature="", out_signature="s")
    def get_config(self):
        self.touch()
        return config_string()

    @dbus.service.method(INTERFACE, in_signature="", out_signature="s")
    def mode_request(self):
        self.touch()
        return config_string()

    @dbus.service.method(INTERFACE, in_signature="", out_signature="s")
    def get_target_state(self):
        self.touch()
        return config_string()

    @dbus.service.method(INTERFACE, in_signature="s", out_signature="s",
                         sender_keyword="sender")
    def set_config(self, mode, sender=None):
        self.touch()
        return self.apply(mode, sender)

    @dbus.service.method(INTERFACE, in_signature="s", out_signature="s",
                         sender_keyword="sender")
    def set_mode(self, mode, sender=None):
        self.touch()
        return self.apply(mode, sender)

    @dbus.service.signal(INTERFACE, signature="s")
    def sig_usb_target_state_ind(self, mode):
        pass

    @dbus.service.signal(INTERFACE, signature="s")
    def sig_usb_current_state_ind(self, mode):
        pass


if __name__ == "__main__":
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    # polkit always lives on the real system bus. On the real system bus this
    # is the same connection; with DBUS_SYSTEM_BUS_ADDRESS redirected (test),
    # FP6_POLKIT_BUS_ADDRESS points back at the real one.
    polkit_addr = os.environ.get("FP6_POLKIT_BUS_ADDRESS")
    polkit_bus = dbus.bus.BusConnection(polkit_addr) if polkit_addr else bus
    name = dbus.service.BusName(BUS_NAME, bus, do_not_queue=True)
    service = UsbModed(bus, polkit_bus)
    log(f"ready, Developer mode is {'on' if adb_enabled() else 'off'}")
    loop = GLib.MainLoop()
    loop.run()
    sys.exit(0)
