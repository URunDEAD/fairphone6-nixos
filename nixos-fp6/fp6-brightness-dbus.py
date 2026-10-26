#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""FP6 manual brightness bridge. No power, idle, sensor or suspend backend."""
import argparse
import os
from pathlib import Path
import signal
import threading
import xml.etree.ElementTree as ET

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

REPOWERD = "com.lomiri.Repowerd"
SCREEN = "com.canonical.Unity.Screen"
REPOWERD_PATH = "/com/lomiri/Repowerd"
SCREEN_PATH = "/com/canonical/Unity/Screen"
PROPERTIES = "org.freedesktop.DBus.Properties"
INTROSPECTABLE = "org.freedesktop.DBus.Introspectable"
LOGIN = "org.freedesktop.login1"
LOGIN_PATH = "/org/freedesktop/login1"
LOGICAL_MAX = 255


class Denied(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.AccessDenied"


class Invalid(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.InvalidArgs"


class Unsupported(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.NotSupported"


class Failed(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.Failed"


class Backlight:
    def __init__(self, directory=Path("/sys/class/backlight/ae94000.dsi.0"),
                 compatible=Path("/proc/device-tree/compatible")):
        # No writable path or hardware-guard override is exposed by the CLI.
        self.directory = Path(directory)
        self.lock = threading.Lock()
        if b"fairphone,fp6" not in Path(compatible).read_bytes().split(b"\0"):
            raise RuntimeError("This backend requires fairphone,fp6")
        self.maximum = self._integer("max_brightness")
        if self.maximum != 4095 or self._text("type") != "raw":
            raise RuntimeError("Unexpected FP6 backlight type or maximum")
        self.initial = self.read()

    def _text(self, name):
        return (self.directory / name).read_text(encoding="ascii").strip()

    def _integer(self, name):
        text = self._text(name)
        if not text.isdecimal():
            raise RuntimeError("Invalid backlight metadata")
        return int(text)

    def read(self):
        with self.lock:
            raw = self._integer("brightness")
            if not 0 <= raw <= self.maximum:
                raise RuntimeError("Backlight readback is outside hardware bounds")
            return (raw * LOGICAL_MAX + self.maximum // 2) // self.maximum

    def write(self, value):
        value = int(value)
        # Zero is a display-power operation, outside this manual slider API.
        if not 1 <= value <= LOGICAL_MAX:
            raise Invalid("Manual brightness must be between 1 and 255")
        with self.lock:
            if self._integer("max_brightness") != self.maximum:
                raise Failed("Backlight maximum changed")
            raw = (value * self.maximum + LOGICAL_MAX // 2) // LOGICAL_MAX
            fd = os.open(self.directory / "brightness", os.O_WRONLY | os.O_TRUNC | os.O_CLOEXEC | os.O_NOFOLLOW)
            try:
                payload = f"{raw}\n".encode("ascii")
                if os.write(fd, payload) != len(payload):
                    raise Failed("Short backlight write")
            finally:
                os.close(fd)
            if self._integer("brightness") != raw:
                raise Failed("Backlight write did not match sysfs readback")
            return (raw * LOGICAL_MAX + self.maximum // 2) // self.maximum


class Authorizer:
    def __init__(self, bus, uid):
        self.bus = bus
        self.uid = uid
        self.daemon = dbus.Interface(bus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"),
                                     "org.freedesktop.DBus")
        self.manager = dbus.Interface(bus.get_object(LOGIN, LOGIN_PATH, introspect=False),
                                      LOGIN + ".Manager")

    def _properties(self, path, interface):
        return dbus.Interface(self.bus.get_object(LOGIN, path, introspect=False), PROPERTIES).GetAll(
            interface, timeout=3)

    @staticmethod
    def _is_local_active(properties, uid):
        return (bool(properties.get("Active", False))
                and not bool(properties.get("Remote", True))
                and str(properties.get("Seat", ("",))[0]) == "seat0"
                and int(properties.get("User", (-1,))[0]) == uid)

    @staticmethod
    def _is_user_manager_pid(pid, uid):
        cgroup = Path(f"/proc/{pid}/cgroup").read_text()
        components = f"/user.slice/user-{uid}.slice/user@{uid}.service/"
        return any(components in line for line in cgroup.splitlines())

    def require(self, sender):
        try:
            uid = int(self.daemon.GetConnectionUnixUser(sender, timeout=3))
            if uid == 0:
                return
            if uid != self.uid:
                raise Denied("Caller is not the configured phone user")
            pid = int(self.daemon.GetConnectionUnixProcessID(sender, timeout=3))
            try:
                path = self.manager.GetSessionByPID(dbus.UInt32(pid), timeout=3)
                if self._is_local_active(self._properties(path, LOGIN + ".Session"), uid):
                    return
                raise Denied("Caller session is not active and local on seat0")
            except dbus.DBusException as error:
                if error.get_dbus_name() != LOGIN + ".NoSessionForPID":
                    raise
            # Indicators run in systemd's per-user manager, outside a login
            # session. Accept that user's services only while their local
            # seat0 session is active. SSH session processes take the branch
            # above and are rejected when Remote=true.
            if not self._is_user_manager_pid(pid, uid):
                raise Denied("Caller is outside the phone user manager")
            seat = self._properties("/org/freedesktop/login1/seat/seat0", LOGIN + ".Seat")
            path = seat["ActiveSession"][1]
            if self._is_local_active(self._properties(path, LOGIN + ".Session"), uid):
                return
            raise Denied("Phone user has no active local seat0 session")
        except Denied:
            raise
        except Exception:
            raise Denied("Could not verify caller's active local session") from None


class Repowerd(dbus.service.Object):
    def __init__(self, bus, backend, authorizer):
        super().__init__(bus, REPOWERD_PATH)
        self.backend = backend
        self.authorizer = authorizer
        self.last = backend.read()

    @dbus.service.method(REPOWERD, in_signature="", out_signature="(iiiib)")
    def getBrightnessParams(self):
        # Logical repowerd scale preserves the existing indicator/GSettings
        # contract; conversion uses the verified hardware maximum above.
        return dbus.Struct((10, 1, LOGICAL_MAX, max(1, self.backend.initial), False), signature="iiiib")

    def write(self, value, sender):
        self.authorizer.require(sender)
        try:
            confirmed = self.backend.write(value)
        except dbus.DBusException:
            raise
        except Exception:
            raise Failed("Backlight write/readback failed") from None
        self.publish(confirmed)

    @dbus.service.method(REPOWERD, in_signature="i", out_signature="", sender_keyword="sender")
    def setUserBrightness(self, value, sender=None):
        self.write(value, sender)

    @dbus.service.method(PROPERTIES, in_signature="ss", out_signature="v")
    def Get(self, interface, name):
        if interface != REPOWERD or name != "brightness":
            raise Invalid("Unknown property")
        return dbus.Int32(self.backend.read(), variant_level=1)

    @dbus.service.method(PROPERTIES, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface != REPOWERD:
            raise Invalid("Unknown interface")
        return {"brightness": dbus.Int32(self.backend.read())}

    @dbus.service.method(PROPERTIES, in_signature="ssv", out_signature="", sender_keyword="sender")
    def Set(self, interface, name, value, sender=None):
        if interface != REPOWERD or name != "brightness" or not isinstance(value, dbus.Int32):
            raise Invalid("Only the integer brightness property is writable")
        self.write(value, sender)

    @dbus.service.signal(PROPERTIES, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated):
        pass

    @dbus.service.method(INTROSPECTABLE, in_signature="", out_signature="s", path_keyword="object_path",
                         connection_keyword="connection")
    def Introspect(self, object_path=None, connection=None):
        node = ET.fromstring(super().Introspect(object_path, connection))
        interface = next(item for item in node.findall("interface") if item.get("name") == REPOWERD)
        ET.SubElement(interface, "property", name="brightness", type="i", access="readwrite")
        return ET.tostring(node, encoding="unicode")

    def publish(self, value):
        if value != self.last:
            self.last = value
            self.PropertiesChanged(REPOWERD, {"brightness": dbus.Int32(value)}, [])

    def poll(self):
        try:
            self.publish(self.backend.read())
        except Exception:
            # Fail closed rather than advertise a stale, successful backend.
            os._exit(1)
        return GLib.SOURCE_CONTINUE


class Screen(dbus.service.Object):
    def __init__(self, bus, repowerd):
        super().__init__(bus, SCREEN_PATH)
        self.repowerd = repowerd

    @dbus.service.method(SCREEN, in_signature="i", out_signature="", sender_keyword="sender")
    def setUserBrightness(self, value, sender=None):
        self.repowerd.write(value, sender)

    @dbus.service.method(SCREEN, in_signature="si", out_signature="b")
    def setScreenPowerMode(self, mode, reason):
        raise Unsupported("Display power and suspend are not implemented by this brightness backend")

    @dbus.service.method(SCREEN, in_signature="ii", out_signature="")
    def setInactivityTimeouts(self, poweroff_timeout, dimmer_timeout):
        raise Unsupported("Idle power management is not implemented")

    @dbus.service.method(SCREEN, in_signature="b", out_signature="")
    def userAutobrightnessEnable(self, enabled):
        raise Unsupported("Automatic brightness is not implemented")

    @dbus.service.method(SCREEN, in_signature="b", out_signature="")
    def setDoubleTapToWakeEnabled(self, enabled):
        raise Unsupported("Double tap to wake is not implemented")


class Service:
    def __init__(self, bus, backend, authorizer):
        # Strict no-queue ownership: never displace a real repowerd instance.
        self.names = [dbus.service.BusName(name, bus, do_not_queue=True) for name in (REPOWERD, SCREEN)]
        self.repowerd = Repowerd(bus, backend, authorizer)
        self.screen = Screen(bus, self.repowerd)
        self.timer = GLib.timeout_add_seconds(2, self.repowerd.poll)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uid", type=int, default=10000)
    args = parser.parse_args()
    if args.uid <= 0:
        parser.error("The phone user UID must be positive")
    DBusGMainLoop(set_as_default=True)
    backend = Backlight()
    bus = dbus.SystemBus()
    service = Service(bus, backend, Authorizer(bus, args.uid))
    loop = GLib.MainLoop()
    for number in (signal.SIGTERM, signal.SIGINT):
        GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, number, lambda: (loop.quit(), GLib.SOURCE_REMOVE)[1])
    print("FP6 manual brightness bridge ready; no startup brightness write", flush=True)
    loop.run()
    return service


if __name__ == "__main__":
    main()
