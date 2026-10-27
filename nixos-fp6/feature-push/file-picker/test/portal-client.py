#!/usr/bin/env python3
"""Call org.freedesktop.portal.FileChooser (the frontend, like an app does)
and print the Response as JSON.

  portal-client.py open|save|save-files|folder [--multiple] [--torrent]
                   [--folder DIR] [--name NAME] [--title T]
"""
import argparse
import json
import sys

from gi.repository import Gio, GLib

ap = argparse.ArgumentParser()
ap.add_argument("mode", choices=["open", "save", "save-files", "folder"])
ap.add_argument("--multiple", action="store_true")
ap.add_argument("--torrent", action="store_true", help="Fragments' filters")
ap.add_argument("--folder")
ap.add_argument("--name")
ap.add_argument("--title", default="")
ap.add_argument("--choices", action="store_true")
ap.add_argument("--timeout", type=int, default=60)
ap.add_argument("--close-after", type=int, default=0, help="cancel via Request.Close after N s")
a = ap.parse_args()

bus = Gio.bus_get_sync(Gio.BusType.SESSION)
token = "fp6test"
sender = bus.get_unique_name()[1:].replace(".", "_")
handle = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
loop = GLib.MainLoop()
result = {}


def on_response(_c, _s, _p, _i, _sig, params):
    code, res = params.unpack()
    result["response"] = code
    result["results"] = res
    loop.quit()


bus.signal_subscribe("org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request",
                     "Response", handle, None, Gio.DBusSignalFlags.NO_MATCH_RULE, on_response)

opts = {"handle_token": GLib.Variant("s", token)}
if a.multiple:
    opts["multiple"] = GLib.Variant("b", True)
if a.mode == "folder":
    opts["directory"] = GLib.Variant("b", True)
if a.torrent:
    flt = [("Torrent files", [(1, "application/x-bittorrent")]), ("All files", [(0, "*")])]
    opts["filters"] = GLib.Variant("a(sa(us))", flt)
    opts["current_filter"] = GLib.Variant("(sa(us))", flt[0])
if a.choices:
    opts["choices"] = GLib.Variant("a(ssa(ss)s)", [
        ("encoding", "Encoding", [("utf8", "UTF-8"), ("latin1", "Western (ISO-8859-1)")], "utf8"),
        ("readonly", "Open read-only", [], "false")])
if a.folder:
    opts["current_folder"] = GLib.Variant("ay", a.folder.encode() + b"\0")
if a.name:
    opts["current_name"] = GLib.Variant("s", a.name)
if a.mode == "save-files":
    opts["files"] = GLib.Variant("aay", [b"photo.jpg\0", b"notes.txt\0"])

method = {"open": "OpenFile", "folder": "OpenFile", "save": "SaveFile", "save-files": "SaveFiles"}[a.mode]
ret = bus.call_sync("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
                    "org.freedesktop.portal.FileChooser", method,
                    GLib.Variant("(ssa{sv})", ("", a.title, opts)), GLib.VariantType("(o)"),
                    Gio.DBusCallFlags.NONE, -1, None)
assert ret.unpack()[0] == handle, ret.unpack()
GLib.timeout_add_seconds(a.timeout, loop.quit)


def close_request():
    bus.call_sync("org.freedesktop.portal.Desktop", handle, "org.freedesktop.portal.Request", "Close",
                  None, None, Gio.DBusCallFlags.NONE, -1, None)
    result["closed"] = True
    GLib.timeout_add(1500, loop.quit)
    return False


if a.close_after:
    GLib.timeout_add_seconds(a.close_after, close_request)
loop.run()
print(json.dumps(result, default=str))
sys.exit(0 if result.get("response") == 0 else 1)
