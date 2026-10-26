#!/usr/bin/env python3
"""Minimal org.freedesktop.URfkill service for the FP6 Lomiri session.

lomiri-indicator-network (flight-mode switch, Wi-Fi kill switch) and the
lomiri-system-settings flight-mode panel talk only to the UBports urfkill
daemon on the system bus. Nixpkgs does not package urfkill, so without a
provider the flight-mode toggle silently does nothing. This daemon implements
the subset of the URfkill 0.6 D-Bus API those clients call:

  /org/freedesktop/URfkill            org.freedesktop.URfkill
      Block(u type, b block) -> b      BlockIdx(u idx, b block) -> b
      FlightMode(b) -> b               IsFlightMode() -> b
      EnumerateDevices() -> ao         IsInhibited() -> b
      Inhibit(s) -> u                  Uninhibit(u)
      signals FlightModeChanged(b), DeviceAdded/Removed/Changed(o)
      properties DaemonVersion(s), KeyControl(b)
  /org/freedesktop/URfkill/{WLAN,BLUETOOTH,WWAN,NFC,...}
      org.freedesktop.URfkill.Killswitch  property state(i), signal StateChanged
  /org/freedesktop/URfkill/devices/<idx>
      org.freedesktop.URfkill.Device  properties index(u) type(u) name(s)
                                      soft(b) hard(b)

rfkill is driven through /dev/rfkill (RFKILL_OP_CHANGE / CHANGE_ALL), exactly
like the kernel rfkill tool; state is learned from the /dev/rfkill event
stream, so external `rfkill` commands and NetworkManager stay in sync.

Flight mode = soft-block every rfkill type listed in --flight-types, remember
the previous per-device soft state, and optionally put the modem offline via
the configured backend:
  --modem-backend ofono         org.ofono.Modem.SetProperty("Online", false)
  --modem-backend modemmanager  org.freedesktop.ModemManager1.Modem.Enable(false)
  --modem-backend none          (default) rfkill only
Leaving flight mode restores the remembered soft states and modem state.
Flight mode persists across reboots in --state-file.

Mutating methods require polkit authorization (actions
org.freedesktop.urfkill.block / org.freedesktop.urfkill.flightmode, which the
NixOS module grants to the active local session like upstream urfkill).
"""

import argparse
import json
import os
import struct
import sys
import syslog

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

BUS_NAME = "org.freedesktop.URfkill"
ROOT_PATH = "/org/freedesktop/URfkill"
IFACE = "org.freedesktop.URfkill"
KS_IFACE = "org.freedesktop.URfkill.Killswitch"
DEV_IFACE = "org.freedesktop.URfkill.Device"
PROPS = "org.freedesktop.DBus.Properties"
VERSION = "0.6.0-fp6"

RFKILL_TYPES = {
    0: "ALL", 1: "WLAN", 2: "BLUETOOTH", 3: "UWB", 4: "WIMAX",
    5: "WWAN", 6: "GPS", 7: "FM", 8: "NFC",
}
TYPE_BY_NAME = {v.lower(): k for k, v in RFKILL_TYPES.items()}
OP_ADD, OP_DEL, OP_CHANGE, OP_CHANGE_ALL = 0, 1, 2, 3
EVENT_FMT = "=IBBBB"          # idx, type, op, soft, hard (struct rfkill_event)
EVENT_SIZE = struct.calcsize(EVENT_FMT)

STATE_NA, STATE_UNBLOCKED, STATE_SOFT, STATE_HARD = -1, 0, 1, 2


def log(msg):
    syslog.syslog(syslog.LOG_INFO, msg)
    print(msg, file=sys.stderr, flush=True)


class AccessDenied(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.URfkill.Error.PermissionDenied"


class Rfkill:
    def __init__(self, path="/dev/rfkill"):
        self.fd = os.open(path, os.O_RDWR | os.O_NONBLOCK | os.O_CLOEXEC)
        self.devices = {}  # idx -> dict(type, soft, hard, name)

    def name_of(self, idx):
        try:
            with open(f"/sys/class/rfkill/rfkill{idx}/name") as f:
                return f.read().strip()
        except OSError:
            return ""

    def read_events(self):
        events = []
        while True:
            # Read exactly one legacy-sized event: the kernel returns one
            # (truncated to the requested size) rfkill_event per read().
            try:
                buf = os.read(self.fd, EVENT_SIZE)
            except BlockingIOError:
                break
            if len(buf) < EVENT_SIZE:
                break
            events.append(struct.unpack(EVENT_FMT, buf[:EVENT_SIZE]))
        return events

    def write(self, idx, rtype, op, soft):
        os.write(self.fd, struct.pack(EVENT_FMT, idx, rtype, op, 1 if soft else 0, 0))

    def block_type(self, rtype, block):
        self.write(0, rtype, OP_CHANGE_ALL, block)

    def block_idx(self, idx, block):
        self.write(idx, 0, OP_CHANGE, block)


class Device(dbus.service.Object):
    def __init__(self, bus, idx, info):
        self.idx = idx
        self.info = info
        super().__init__(bus, f"{ROOT_PATH}/devices/{idx}")

    def props(self):
        return {
            "index": dbus.UInt32(self.idx),
            "type": dbus.UInt32(self.info["type"]),
            "name": dbus.String(self.info["name"]),
            "soft": dbus.Boolean(self.info["soft"]),
            "hard": dbus.Boolean(self.info["hard"]),
            "platform": dbus.Boolean(False),
        }

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return self.props()[prop]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.props() if iface in ("", DEV_IFACE) else {}

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    @dbus.service.signal(DEV_IFACE, signature="")
    def Changed(self):
        pass


class Killswitch(dbus.service.Object):
    def __init__(self, bus, rtype, daemon):
        self.rtype = rtype
        self.daemon = daemon
        self.last = None
        super().__init__(bus, f"{ROOT_PATH}/{RFKILL_TYPES[rtype]}")

    def state(self):
        devs = [d for d in self.daemon.rfkill.devices.values() if d["type"] == self.rtype]
        if not devs:
            return STATE_NA
        if any(d["hard"] for d in devs):
            return STATE_HARD
        if all(d["soft"] for d in devs):
            return STATE_SOFT
        return STATE_UNBLOCKED

    def refresh(self):
        s = self.state()
        if s != self.last:
            self.last = s
            self.PropertiesChanged(KS_IFACE, {"state": dbus.Int32(s)}, [])
            self.StateChanged()

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        if prop != "state":
            raise dbus.DBusException("org.freedesktop.DBus.Error.UnknownProperty")
        return dbus.Int32(self.state())

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return {"state": dbus.Int32(self.state())} if iface in ("", KS_IFACE) else {}

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    @dbus.service.signal(KS_IFACE, signature="")
    def StateChanged(self):
        pass


class Daemon(dbus.service.Object):
    def __init__(self, bus, args):
        self.bus = bus
        self.args = args
        self.rfkill = Rfkill(args.rfkill_device)
        self.flight_types = [TYPE_BY_NAME[t] for t in args.flight_types]
        self.dev_objs = {}
        self.inhibit_cookie = 0
        self.inhibitors = {}
        self.persist = self.load_state()
        super().__init__(bus, ROOT_PATH)
        self.killswitches = {t: Killswitch(bus, t, self) for t in RFKILL_TYPES if t != 0}
        self.handle_events(self.rfkill.read_events())
        GLib.io_add_watch(self.rfkill.fd, GLib.PRIORITY_DEFAULT, GLib.IO_IN, self.on_io)
        if self.persist.get("flight_mode"):
            log("restoring persisted flight mode")
            GLib.idle_add(lambda: self.apply_flight(True, restore=True) and False)

    # ---------- persistence ----------
    def load_state(self):
        try:
            with open(self.args.state_file) as f:
                data = json.load(f)
            return data if isinstance(data, dict) else {}
        except (OSError, ValueError):
            return {}

    def save_state(self):
        tmp = self.args.state_file + ".tmp"
        os.makedirs(os.path.dirname(self.args.state_file), exist_ok=True)
        with open(tmp, "w") as f:
            json.dump(self.persist, f)
        os.replace(tmp, self.args.state_file)

    # ---------- rfkill events ----------
    def on_io(self, fd, cond):
        self.handle_events(self.rfkill.read_events())
        return True

    def handle_events(self, events):
        for idx, rtype, op, soft, hard in events:
            devs = self.rfkill.devices
            if op == OP_DEL:
                devs.pop(idx, None)
                obj = self.dev_objs.pop(idx, None)
                if obj:
                    path = obj.__dbus_object_path__
                    obj.remove_from_connection()
                    self.DeviceRemoved(path)
            elif op in (OP_ADD, OP_CHANGE):
                info = {"type": rtype, "soft": bool(soft), "hard": bool(hard),
                        "name": self.rfkill.name_of(idx)}
                devs[idx] = info
                if idx not in self.dev_objs:
                    self.dev_objs[idx] = Device(self.bus, idx, info)
                    self.DeviceAdded(self.dev_objs[idx].__dbus_object_path__)
                else:
                    obj = self.dev_objs[idx]
                    obj.info = info
                    obj.PropertiesChanged(DEV_IFACE, obj.props(), [])
                    obj.Changed()
                    self.DeviceChanged(obj.__dbus_object_path__)
        for ks in getattr(self, "killswitches", {}).values():
            ks.refresh()

    # ---------- authorization ----------
    def authorize(self, sender, action):
        if sender is None:
            return
        try:
            authority = self.bus.get_object("org.freedesktop.PolicyKit1",
                                            "/org/freedesktop/PolicyKit1/Authority")
            subject = ("system-bus-name", {"name": dbus.String(sender, variant_level=1)})
            ok, _challenge, _details = authority.CheckAuthorization(
                subject, action, dbus.Dictionary({}, signature="ss"),
                dbus.UInt32(1), "", dbus_interface="org.freedesktop.PolicyKit1.Authority",
                timeout=120)
        except dbus.DBusException as e:
            log(f"polkit check failed for {sender}: {e}")
            raise AccessDenied("authorization check failed")
        if not ok:
            raise AccessDenied(f"{action} not authorized")

    # ---------- modem ----------
    def modem_set_online(self, online, only=None):
        """Set modems on/offline; returns the object paths changed.

        Going online only touches the modems this daemon took offline
        (``only``), so a modem the cellular stack had disabled stays so."""
        backend = self.args.modem_backend
        changed = []
        try:
            if backend == "ofono":
                mgr = self.bus.get_object("org.ofono", "/")
                for path, props in mgr.GetModems(dbus_interface="org.ofono.Manager"):
                    if not props.get("Powered", False):
                        continue
                    if only is not None and str(path) not in only:
                        continue
                    modem = dbus.Interface(self.bus.get_object("org.ofono", path), "org.ofono.Modem")
                    if bool(props.get("Online", False)) != online:
                        modem.SetProperty("Online", dbus.Boolean(online, variant_level=1), timeout=30)
                        changed.append(str(path))
            elif backend == "modemmanager":
                om = self.bus.get_object("org.freedesktop.ModemManager1", "/org/freedesktop/ModemManager1")
                objs = om.GetManagedObjects(dbus_interface="org.freedesktop.DBus.ObjectManager")
                for path, ifaces in objs.items():
                    if "org.freedesktop.ModemManager1.Modem" not in ifaces:
                        continue
                    if only is not None and str(path) not in only:
                        continue
                    state = int(ifaces["org.freedesktop.ModemManager1.Modem"].get("State", 0))
                    # MM_MODEM_STATE_ENABLED == 6; below that it is not enabled.
                    if not online and state < 6:
                        continue
                    modem = dbus.Interface(self.bus.get_object("org.freedesktop.ModemManager1", path),
                                           "org.freedesktop.ModemManager1.Modem")
                    modem.Enable(dbus.Boolean(online), timeout=60)
                    changed.append(str(path))
        except dbus.DBusException as e:
            log(f"modem backend {backend}: {e.get_dbus_name()}: {e.get_dbus_message()}")
        if changed:
            log(f"modem online={online} via {backend}: {changed}")
        return changed

    # ---------- flight mode ----------
    def apply_flight(self, enable, restore=False):
        if enable:
            if not restore:
                self.persist["saved"] = {
                    str(i): d["soft"] for i, d in self.rfkill.devices.items()
                    if d["type"] in self.flight_types}
            for t in self.flight_types:
                self.rfkill.block_type(t, True)
            offline = self.modem_set_online(False)
            if not restore:
                self.persist["modems"] = offline
            else:
                self.persist["modems"] = sorted(set(self.persist.get("modems", [])) | set(offline))
        else:
            saved = self.persist.get("saved", {})
            for t in self.flight_types:
                self.rfkill.block_type(t, False)
            for idx, soft in saved.items():
                if soft and int(idx) in self.rfkill.devices:
                    self.rfkill.block_idx(int(idx), True)
            self.modem_set_online(True, only=set(self.persist.get("modems", [])))
            self.persist["saved"] = {}
            self.persist["modems"] = []
        self.persist["flight_mode"] = bool(enable)
        self.save_state()
        self.handle_events(self.rfkill.read_events())
        self.FlightModeChanged(bool(enable))
        log(f"flight mode {'on' if enable else 'off'}")
        return True

    # ---------- D-Bus API ----------
    @dbus.service.method(IFACE, in_signature="ub", out_signature="b", sender_keyword="sender")
    def Block(self, rtype, block, sender=None):
        self.authorize(sender, "org.freedesktop.urfkill.block")
        if rtype not in RFKILL_TYPES:
            return False
        self.rfkill.block_type(int(rtype), bool(block))
        return True

    @dbus.service.method(IFACE, in_signature="ub", out_signature="b", sender_keyword="sender")
    def BlockIdx(self, idx, block, sender=None):
        self.authorize(sender, "org.freedesktop.urfkill.block")
        if int(idx) not in self.rfkill.devices:
            return False
        self.rfkill.block_idx(int(idx), bool(block))
        return True

    @dbus.service.method(IFACE, in_signature="", out_signature="ao")
    def EnumerateDevices(self):
        return dbus.Array([o.__dbus_object_path__ for _, o in sorted(self.dev_objs.items())],
                          signature="o")

    @dbus.service.method(IFACE, in_signature="b", out_signature="b", sender_keyword="sender")
    def FlightMode(self, enable, sender=None):
        self.authorize(sender, "org.freedesktop.urfkill.flightmode")
        if bool(enable) == bool(self.persist.get("flight_mode")):
            return True
        return self.apply_flight(bool(enable))

    @dbus.service.method(IFACE, in_signature="", out_signature="b")
    def IsFlightMode(self):
        return bool(self.persist.get("flight_mode"))

    @dbus.service.method(IFACE, in_signature="", out_signature="b")
    def IsInhibited(self):
        return bool(self.inhibitors)

    @dbus.service.method(IFACE, in_signature="s", out_signature="u", sender_keyword="sender")
    def Inhibit(self, reason, sender=None):
        self.inhibit_cookie += 1
        self.inhibitors[self.inhibit_cookie] = (sender, str(reason))
        return dbus.UInt32(self.inhibit_cookie)

    @dbus.service.method(IFACE, in_signature="u", out_signature="")
    def Uninhibit(self, cookie):
        self.inhibitors.pop(int(cookie), None)

    @dbus.service.signal(IFACE, signature="b")
    def FlightModeChanged(self, enabled):
        pass

    @dbus.service.signal(IFACE, signature="o")
    def DeviceAdded(self, path):
        pass

    @dbus.service.signal(IFACE, signature="o")
    def DeviceRemoved(self, path):
        pass

    @dbus.service.signal(IFACE, signature="o")
    def DeviceChanged(self, path):
        pass

    @dbus.service.signal(IFACE, signature="i")
    def UrfkeyPressed(self, keycode):
        pass

    def root_props(self):
        return {"DaemonVersion": dbus.String(VERSION), "KeyControl": dbus.Boolean(False)}

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return self.root_props()[prop]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.root_props() if iface in ("", IFACE) else {}


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--flight-types", default="wlan,bluetooth,nfc,wwan,gps,fm,uwb,wimax",
                    type=lambda s: [t.strip().lower() for t in s.split(",") if t.strip()])
    ap.add_argument("--modem-backend", choices=["none", "ofono", "modemmanager"], default="none")
    ap.add_argument("--state-file", default="/var/lib/fp6-urfkill/state.json")
    # Test hooks for the build-time check: a session bus and a FIFO standing in
    # for /dev/rfkill. Authorization is never bypassed; on a bus without polkit
    # every mutating call is refused.
    ap.add_argument("--bus", choices=["system", "session"], default="system")
    ap.add_argument("--rfkill-device", default="/dev/rfkill")
    args = ap.parse_args()
    for t in args.flight_types:
        if t not in TYPE_BY_NAME or t == "all":
            ap.error(f"unknown rfkill type {t}")
    syslog.openlog("fp6-urfkill")
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus() if args.bus == "system" else dbus.SessionBus()
    Daemon(bus, args)
    # Claim the name only after objects exist so clients never see a half-ready service.
    name = dbus.service.BusName(BUS_NAME, bus, do_not_queue=True)  # noqa: F841
    log(f"{BUS_NAME} ready (modem backend: {args.modem_backend})")
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
