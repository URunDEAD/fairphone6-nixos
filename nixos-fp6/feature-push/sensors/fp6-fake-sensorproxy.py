#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Scripted net.hadess.SensorProxy stand-in for fp6-lomiri-powerd tests."""
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

IFACE, PATH, PROPS = "net.hadess.SensorProxy", "/net/hadess/SensorProxy", "org.freedesktop.DBus.Properties"


class Proxy(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, PATH)
        self.props = {"HasAmbientLight": True, "HasProximity": True, "LightLevel": 0.0,
                      "LightLevelUnit": "lux", "ProximityNear": False}
        self.claims = {"light": 0, "prox": 0}

    @dbus.service.method(IFACE)
    def ClaimLight(self): self.claims["light"] += 1

    @dbus.service.method(IFACE)
    def ReleaseLight(self): self.claims["light"] -= 1

    @dbus.service.method(IFACE)
    def ClaimProximity(self): self.claims["prox"] += 1

    @dbus.service.method(IFACE)
    def ReleaseProximity(self): self.claims["prox"] -= 1

    @dbus.service.method(IFACE, out_signature="ii")
    def Claims(self): return (self.claims["light"], self.claims["prox"])

    @dbus.service.method(IFACE, in_signature="sv")
    def Inject(self, name, value):
        self.props[name] = value
        self.PropertiesChanged(IFACE, {name: value}, [])

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, interface, name): return self.props[name]

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated): pass


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName(IFACE, bus)
proxy = Proxy(bus)
GLib.MainLoop().run()
