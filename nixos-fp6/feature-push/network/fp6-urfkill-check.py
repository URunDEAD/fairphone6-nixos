#!/usr/bin/env python3
"""Build-time functional check for fp6-urfkill (no hardware, no polkit).

Runs the daemon on a private session bus with a FIFO in place of /dev/rfkill,
pre-seeded with three rfkill ADD events (wlan, bluetooth, soft-blocked nfc).
Verifies the read API used by lomiri-indicator-network / system-settings and
that mutating calls are refused when polkit is unreachable.
Usage: fp6-urfkill-check.py <path-to-fp6-urfkill>
"""
import os
import struct
import subprocess
import sys
import tempfile
import time

import dbus

daemon = sys.argv[1]
tmp = tempfile.mkdtemp()
fifo = os.path.join(tmp, "rfkill")
os.mkfifo(fifo)
holder = os.open(fifo, os.O_RDWR | os.O_NONBLOCK)
for idx, rtype, soft in ((0, 1, 0), (1, 2, 0), (2, 8, 1)):
    os.write(holder, struct.pack("=IBBBB", idx, rtype, 0, soft, 0))

bus_proc = subprocess.Popen(
    ["dbus-daemon", "--config-file", os.environ.get("DBUS_SESSION_CONF", "/etc/dbus-1/session.conf"),
     "--nofork", "--print-address=1"],
    stdout=subprocess.PIPE, text=True)
address = bus_proc.stdout.readline().strip()
if not address:
    sys.exit("private dbus-daemon failed to start")
os.environ["DBUS_SESSION_BUS_ADDRESS"] = address
proc = subprocess.Popen([daemon, "--bus", "session", "--rfkill-device", fifo,
                         "--state-file", os.path.join(tmp, "state.json")])
try:
    bus = dbus.SessionBus()
    for _ in range(100):
        if bus.name_has_owner("org.freedesktop.URfkill"):
            break
        time.sleep(0.1)
    else:
        sys.exit("daemon did not claim org.freedesktop.URfkill")
    root = bus.get_object("org.freedesktop.URfkill", "/org/freedesktop/URfkill")
    api = dbus.Interface(root, "org.freedesktop.URfkill")
    assert api.IsFlightMode() is not None and not api.IsFlightMode()
    devices = api.EnumerateDevices()
    assert len(devices) == 3, devices

    def ks(name):
        o = bus.get_object("org.freedesktop.URfkill", f"/org/freedesktop/URfkill/{name}")
        return int(o.Get("org.freedesktop.URfkill.Killswitch", "state",
                         dbus_interface="org.freedesktop.DBus.Properties"))

    assert ks("WLAN") == 0, ks("WLAN")
    assert ks("BLUETOOTH") == 0
    assert ks("NFC") == 1
    assert ks("WWAN") == -1
    version = root.Get("org.freedesktop.URfkill", "DaemonVersion",
                       dbus_interface="org.freedesktop.DBus.Properties")
    assert str(version).endswith("-fp6"), version
    for call in (lambda: api.FlightMode(True), lambda: api.Block(dbus.UInt32(1), True)):
        try:
            call()
        except dbus.DBusException as e:
            assert e.get_dbus_name() == "org.freedesktop.URfkill.Error.PermissionDenied", e
        else:
            sys.exit("mutating call was not refused without polkit")
    assert not api.IsFlightMode()
    print("fp6-urfkill check: PASS (read API, killswitch states, polkit-gated writes)")
finally:
    proc.terminate()
    proc.wait(10)
    bus_proc.terminate()
    bus_proc.wait(10)
