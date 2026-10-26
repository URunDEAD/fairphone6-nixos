# SPDX-License-Identifier: MIT
"""Private-bus integration tests; all writes target temporary regular files."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib
import fp6_brightness as bridge


def assert_error(name, function, *args):
    try:
        function(*args)
    except dbus.DBusException as error:
        assert error.get_dbus_name() == name, error
    else:
        raise AssertionError(f"Expected {name}")


class LoginManager(dbus.service.Object):
    @dbus.service.method(bridge.LOGIN + ".Manager", in_signature="u", out_signature="o")
    def GetSessionByPID(self, pid):
        return "/org/freedesktop/login1/session/test"


class Session(dbus.service.Object):
    def __init__(self, bus, root):
        super().__init__(bus, "/org/freedesktop/login1/session/test")
        self.root = root

    @dbus.service.method(bridge.PROPERTIES, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        state = json.loads((self.root / "session.json").read_text())
        return {
            "Active": dbus.Boolean(state["active"]),
            "Remote": dbus.Boolean(state["remote"]),
            "Seat": dbus.Struct(("seat0", dbus.ObjectPath("/org/freedesktop/login1/seat/seat0"))),
            "User": dbus.Struct((dbus.UInt32(os.getuid()), dbus.ObjectPath("/org/freedesktop/login1/user/test"))),
        }


def child(mode, root):
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    if mode == "login":
        name = dbus.service.BusName(bridge.LOGIN, bus, do_not_queue=True)
        manager = LoginManager(bus, bridge.LOGIN_PATH)
        session = Session(bus, root)
    else:
        backend = bridge.Backlight(root / "backlight", root / "compatible")
        service = bridge.Service(bus, backend, bridge.Authorizer(bus, os.getuid()))
    (root / (mode + ".ready")).touch()
    GLib.MainLoop().run()


def wait_ready(process, path):
    deadline = time.monotonic() + 5
    while not path.exists():
        assert process.poll() is None, "Fixture exited early"
        assert time.monotonic() < deadline, "Fixture startup timed out"
        time.sleep(0.02)


def test_authorizer():
    # Exercise both explicit caller UID rejection and local-session predicate
    # independently of the Nix builder's host UID (including root test runs).
    class Daemon:
        def GetConnectionUnixUser(self, sender, **kwargs):
            return 12345
        def GetConnectionUnixProcessID(self, sender, **kwargs):
            return 123
    auth = bridge.Authorizer.__new__(bridge.Authorizer)
    auth.daemon, auth.uid = Daemon(), 54321
    assert_error("org.freedesktop.DBus.Error.AccessDenied", auth.require, ":1.test")
    local = {"Active": True, "Remote": False, "Seat": ("seat0", "/seat"), "User": (12345, "/user")}
    assert auth._is_local_active(local, 12345)
    assert not auth._is_local_active({**local, "Active": False}, 12345)
    assert not auth._is_local_active({**local, "Remote": True}, 12345)
    assert not auth._is_local_active(local, 54321)
    auth.uid = 12345
    class NoSession(dbus.DBusException):
        _dbus_error_name = bridge.LOGIN + ".NoSessionForPID"
    class Manager:
        no_session = False
        def GetSessionByPID(self, pid, **kwargs):
            if self.no_session:
                raise NoSession("No session for test user-manager process")
            return "/session"
    auth.manager = Manager()
    state = dict(local)
    auth._properties = lambda path, interface: ({"ActiveSession": ("test", "/session")}
                                               if interface.endswith(".Seat") else state)
    auth.require(":1.test")
    for key, value in (("Active", False), ("Remote", True), ("Seat", ("seat1", "/seat")),
                       ("User", (99999, "/user"))):
        state = {**local, key: value}
        assert_error("org.freedesktop.DBus.Error.AccessDenied", auth.require, ":1.test")
    state = dict(local)
    auth.manager.no_session = True
    auth._is_user_manager_pid = lambda pid, uid: True
    auth.require(":1.user-manager")
    auth._is_user_manager_pid = lambda pid, uid: False
    assert_error("org.freedesktop.DBus.Error.AccessDenied", auth.require, ":1.unmanaged")
    auth._is_user_manager_pid = lambda pid, uid: True
    state = {**local, "Remote": True}
    assert_error("org.freedesktop.DBus.Error.AccessDenied", auth.require, ":1.remote-seat")
    state = {**local, "Active": False}
    assert_error("org.freedesktop.DBus.Error.AccessDenied", auth.require, ":1.inactive-seat")


def run():
    DBusGMainLoop(set_as_default=True)
    test_authorizer()
    processes = []
    with tempfile.TemporaryDirectory(prefix="fp6-brightness-test-") as directory:
        root = Path(directory)
        panel = root / "backlight"
        panel.mkdir()
        (root / "compatible").write_bytes(b"fairphone,fp6\0qcom,milos\0")
        (panel / "max_brightness").write_text("4095\n")
        (panel / "brightness").write_text("1365\n")
        (panel / "type").write_text("raw\n")
        (root / "session.json").write_text(json.dumps({"active": True, "remote": False}))
        for path, bad in [(root / "compatible", "other,phone"), (panel / "max_brightness", "255"),
                          (panel / "type", "firmware"), (panel / "brightness", "4096")]:
            original = path.read_bytes()
            path.write_text(bad)
            try:
                bridge.Backlight(panel, root / "compatible")
            except RuntimeError:
                pass
            else:
                raise AssertionError("Hardware guard failed")
            path.write_bytes(original)
        try:
            for mode in ("login", "bridge"):
                process = subprocess.Popen([sys.executable, __file__, mode, str(root)])
                processes.append(process)
                wait_ready(process, root / (mode + ".ready"))
            assert (panel / "brightness").read_text() == "1365\n", "Startup changed brightness"
            bus = dbus.SessionBus()
            object_ = bus.get_object(bridge.REPOWERD, bridge.REPOWERD_PATH)
            power = dbus.Interface(object_, bridge.REPOWERD)
            prop = dbus.Interface(object_, bridge.PROPERTIES)
            screen = dbus.Interface(bus.get_object(bridge.SCREEN, bridge.SCREEN_PATH), bridge.SCREEN)
            params = tuple(power.getBrightnessParams())
            assert params == (10, 1, 255, 85, False), params
            assert int(prop.Get(bridge.REPOWERD, "brightness")) == 85
            xml = dbus.Interface(object_, bridge.INTROSPECTABLE).Introspect()
            assert 'name="brightness" type="i" access="readwrite"' in xml
            updates = []
            bus.add_signal_receiver(lambda interface, changed, invalidated: updates.append(dict(changed)),
                                    dbus_interface=bridge.PROPERTIES, signal_name="PropertiesChanged",
                                    path=bridge.REPOWERD_PATH)
            for logical, raw in [(1, 16), (255, 4095), (85, 1365)]:
                screen.setUserBrightness(dbus.Int32(logical))
                assert int((panel / "brightness").read_text()) == raw
                assert int(prop.Get(bridge.REPOWERD, "brightness")) == logical
            for invalid in (-1, 0, 256, 2147483647):
                assert_error("org.freedesktop.DBus.Error.InvalidArgs", screen.setUserBrightness, dbus.Int32(invalid))
                assert int((panel / "brightness").read_text()) == 1365
            prop.Set(bridge.REPOWERD, "brightness", dbus.Int32(127))
            assert int((panel / "brightness").read_text()) == 2039
            assert_error("org.freedesktop.DBus.Error.InvalidArgs", prop.Set, bridge.REPOWERD, "brightness", dbus.String("1"))
            for function, args in [(screen.setScreenPowerMode, ("off", 0)),
                                   (screen.userAutobrightnessEnable, (False,)),
                                   (screen.setInactivityTimeouts, (60, 10)),
                                   (screen.setDoubleTapToWakeEnabled, (False,))]:
                assert_error("org.freedesktop.DBus.Error.NotSupported", function, *args)
            (panel / "max_brightness").write_text("4096\n")
            assert_error("org.freedesktop.DBus.Error.Failed", screen.setUserBrightness, dbus.Int32(128))
            assert int((panel / "brightness").read_text()) == 2039
            (panel / "max_brightness").write_text("4095\n")
            # The real D-Bus UID is used. Root deliberately retains an admin
            # override, so root-only fixture runs cover the predicates above.
            if os.getuid() != 0:
                for state in ({"active": False, "remote": False}, {"active": True, "remote": True}):
                    (root / "session.json").write_text(json.dumps(state))
                    assert_error("org.freedesktop.DBus.Error.AccessDenied", screen.setUserBrightness, dbus.Int32(128))
                    assert int((panel / "brightness").read_text()) == 2039
            (root / "session.json").write_text(json.dumps({"active": True, "remote": False}))
            # Readback updates also cover out-of-band sysfs changes.
            (panel / "brightness").write_text("2730\n")
            deadline = time.monotonic() + 5
            context = GLib.MainContext.default()
            while not any(int(item.get("brightness", -1)) == 170 for item in updates):
                while context.pending():
                    context.iteration(False)
                assert time.monotonic() < deadline, updates
                time.sleep(0.02)
            assert any(int(item.get("brightness", -1)) == 1 for item in updates)
            assert any(int(item.get("brightness", -1)) == 255 for item in updates)
            collision = subprocess.run([sys.executable, __file__, "bridge", str(root)], capture_output=True)
            assert collision.returncode != 0, "Duplicate service took existing name"
            print("PASS hardware guards; no startup write; D-Bus params/properties/introspection; min/max scaling; invalid values; readback signals; active/local authorization; unsupported power APIs; exclusive names")
        finally:
            for process in reversed(processes):
                process.terminate()
                process.wait(timeout=5)


if __name__ == "__main__":
    if len(sys.argv) == 3:
        child(sys.argv[1], Path(sys.argv[2]))
    else:
        run()
