#!/usr/bin/env python3
"""Real telepathy-ofono + ofono-qt, synthetic oFono, private bus only.

Invoke through dbus-run-session, with BINARY and voice-only|sim-registered.
No account, carrier, modem, radio, identifier, or host service is accessed.
"""
import json
import os
import subprocess
import sys
import threading
import time
from gi.repository import Gio, GLib


binary, scenario = sys.argv[1:]
assert scenario in ("voice-only", "sim-registered")
address = os.environ["DBUS_SESSION_BUS_ADDRESS"]
assert address.startswith("unix:"), address
os.environ["DBUS_SYSTEM_BUS_ADDRESS"] = address
os.environ["QT_QPA_PLATFORM"] = "offscreen"
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
loop = GLib.MainLoop()
thread = threading.Thread(target=loop.run, daemon=True)
thread.start()
modem_path = "/synthetic"
contracts = ["VoiceCallManager"]
if scenario == "sim-registered":
    contracts += ["SimManager", "NetworkRegistration", "MessageManager",
                  "MessageWaiting", "CallVolume", "SupplementaryServices", "CellBroadcast"]
modem = {
    "Powered": GLib.Variant("b", True), "Online": GLib.Variant("b", True),
    "Serial": GLib.Variant("s", "SYNTHETIC-NOT-HARDWARE"),
    "Interfaces": GLib.Variant("as", ["org.ofono." + i for i in contracts]),
}
properties = {
    "org.ofono.Modem": modem,
    "org.ofono.SimManager": {
        "Present": GLib.Variant("b", True), "PinRequired": GLib.Variant("s", "none"),
        "SubscriberNumbers": GLib.Variant("as", []),
        "MobileCountryCode": GLib.Variant("s", "001"),
        "MobileNetworkCode": GLib.Variant("s", "01"),
    },
    "org.ofono.NetworkRegistration": {
        "Status": GLib.Variant("s", "registered"), "Name": GLib.Variant("s", "Synthetic Network"),
        "MobileCountryCode": GLib.Variant("s", "001"), "MobileNetworkCode": GLib.Variant("s", "01"),
        "Technology": GLib.Variant("s", "lte"), "Strength": GLib.Variant("y", 75),
    },
    "org.ofono.VoiceCallManager": {"EmergencyNumbers": GLib.Variant("as", ["112"])},
}
calls = []


def method(_bus, _sender, obj, iface, name, _params, invocation):
    calls.append([obj, iface, name])
    if name == "GetModems":
        invocation.return_value(GLib.Variant("(a(oa{sv}))", ([(modem_path, modem)],)))
    elif name == "GetProperties":
        invocation.return_value(GLib.Variant("(a{sv})", (properties.get(iface, {}),)))
    elif name in ("GetCalls", "GetMessages"):
        invocation.return_value(GLib.Variant("(a(oa{sv}))", ([],)))
    else:
        invocation.return_dbus_error("org.ofono.Error.NotImplemented", "Synthetic read-only fixture")


def register(path, iface, extra=""):
    xml = f'''<node><interface name="{iface}">
      <method name="GetProperties"><arg type="a{{sv}}" direction="out"/></method>
      <signal name="PropertyChanged"><arg type="s"/><arg type="v"/></signal>
      {extra}</interface></node>'''
    info = Gio.DBusNodeInfo.new_for_xml(xml)
    return bus.register_object(path, info.interfaces[0], method, None, None)


object_ids = [register("/", "org.ofono.Manager", '''
  <method name="GetModems"><arg type="a(oa{sv})" direction="out"/></method>
  <signal name="ModemAdded"><arg type="o"/><arg type="a{sv}"/></signal>
  <signal name="ModemRemoved"><arg type="o"/></signal>''')]
for contract in ["Modem"] + contracts:
    extra = ""
    if contract in ("VoiceCallManager", "MessageManager"):
        method_name = "GetCalls" if contract == "VoiceCallManager" else "GetMessages"
        extra = f'<method name="{method_name}"><arg type="a(oa{{sv}})" direction="out"/></method>'
    object_ids.append(register(modem_path, "org.ofono." + contract, extra))
name_id = Gio.bus_own_name_on_connection(bus, "org.ofono", Gio.BusNameOwnerFlags.NONE, None, None)
child = subprocess.Popen([binary], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)


def call(name, path, iface, method_name, parameters=None):
    return bus.call_sync(name, path, iface, method_name, parameters, None,
                         Gio.DBusCallFlags.NO_AUTO_START, 2000, None).unpack()


try:
    manager = "org.freedesktop.Telepathy.ConnectionManager.ofono"
    manager_path = "/org/freedesktop/Telepathy/ConnectionManager/ofono"
    deadline = time.monotonic() + 8
    while True:
        try:
            connection_name, connection_path = call(
                manager, manager_path, "org.freedesktop.Telepathy.ConnectionManager",
                "RequestConnection", GLib.Variant("(sa{sv})", ("ofono", {
                    "modem-objpath": GLib.Variant("s", modem_path),
                })))
            break
        except GLib.Error:
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.05)
    connection_iface = "org.freedesktop.Telepathy.Connection"
    call(connection_name, connection_path, connection_iface, "Connect")
    self_handle = call(connection_name, connection_path, connection_iface, "GetSelfHandle")[0]
    expected_status = "nosim" if scenario == "voice-only" else "registered"
    deadline = time.monotonic() + 6
    while True:
        presence = call(connection_name, connection_path, connection_iface + ".Interface.SimplePresence",
                        "GetPresences", GLib.Variant("(au)", ([self_handle],)))[0][self_handle]
        if presence[1] == expected_status:
            break
        if time.monotonic() >= deadline:
            raise AssertionError((expected_status, presence))
        time.sleep(0.05)
    assert presence[0] == (1 if scenario == "voice-only" else 2), presence
    print(json.dumps({"result": "PASS", "scenario": scenario,
                      "presence": presence, "synthetic_requests": calls,
                      "scope": "Private bus; synthetic modem; no host system bus or radio"}))
finally:
    child.terminate()
    try:
        log = child.communicate(timeout=3)[0]
    except subprocess.TimeoutExpired:
        child.kill()
        log = child.communicate()[0]
    print(log, file=sys.stderr)
    Gio.bus_unown_name(name_id)
    for object_id in object_ids:
        bus.unregister_object(object_id)
    loop.quit()
