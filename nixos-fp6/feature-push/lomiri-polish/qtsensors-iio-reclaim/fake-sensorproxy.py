#!/usr/bin/env python3
"""Minimal net.hadess.SensorProxy accelerometer stand-in (test only)."""
import sys
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

IFACE, PATH, PROPS = "net.hadess.SensorProxy", "/net/hadess/SensorProxy", "org.freedesktop.DBus.Properties"


class Proxy(dbus.service.Object):
    def __init__(self, bus, orientation, late):
        super().__init__(bus, PATH)
        self.props = {"HasAccelerometer": not late, "AccelerometerOrientation": orientation}
        self.claims = 0
        if late:
            GLib.timeout_add(300, self.probe)

    def probe(self):
        self.Inject("HasAccelerometer", True)
        return False

    @dbus.service.method(IFACE)
    def ClaimAccelerometer(self): self.claims += 1

    @dbus.service.method(IFACE)
    def ReleaseAccelerometer(self): self.claims -= 1

    @dbus.service.method(IFACE, out_signature="i")
    def Claims(self): return self.claims

    @dbus.service.method(IFACE, in_signature="sv")
    def Inject(self, name, value):
        self.props[name] = value
        self.PropertiesChanged(IFACE, {name: value}, [])

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, interface, name): return self.props[name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface): return self.props

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated): pass


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
proxy = Proxy(bus, sys.argv[1], "--late" in sys.argv[2:])
name = dbus.service.BusName(IFACE, bus)  # own the name only once objects exist
GLib.MainLoop().run()
