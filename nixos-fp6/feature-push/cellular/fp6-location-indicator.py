"""Location indicator for Lomiri on FP6 (stand-in for lomiri-indicator-location).

Owns com.lomiri.indicator.location on the session bus and exports what both
consumers use:
  * the status-bar indicator: menu /com/lomiri/indicator/location/phone
    (org.gtk.Menus) with a root item and a "Location detection" switch;
  * System Settings > Security & Privacy > Location (Location.qml): the
    action group /com/lomiri/indicator/location (org.gtk.Actions), stateful
    boolean action "location-detection-enabled" (and "gps-detection-enabled").
Turning it on starts the system unit fp6-gnss-nmea.service (GeoClue's NMEA
socket -> fp6-loc-nmea -> modem LOC session on demand); turning it off stops
the unit, which ends any running LOC session (Stop sent, engine idles).
Default is OFF; the choice is kept in $XDG_CONFIG_HOME/fp6-location/enabled.
Starting/stopping that one unit is allowed for the radio group by polkit.

It is also the user's GeoClue agent (org.freedesktop.GeoClue2.Agent, id
"fp6-location-indicator", whitelisted in geoclue.conf): GeoClue refuses
non-system apps when no agent is registered. AuthorizeApp grants the
requested accuracy while location detection is on and refuses it when off;
MaxAccuracyLevel follows the toggle (0 = none).

Per-app location permission (app-permissions module, media option; the
unit then sets FP6_LOCATION_PERMISSIONS=1): GeoClue (patched to name host
apps by AppArmor profile / app cgroup and to ask the agent about them)
calls AuthorizeApp with that app id; while location detection is on, the
answer comes from fp6-permd (AuthorizeApp "location": the app's setting, or
the Allow / Only this time / Don't allow prompt), asked asynchronously so a
prompt does not block the indicator. Only GeoClue may call AuthorizeApp.

Privacy (privacy-lock module):
  * every AuthorizeApp answer is recorded in
    $XDG_RUNTIME_DIR/fp6-privacy/location-apps.json (last 20: desktop id,
    time, granted) for the status-bar in-use indicator (fp6-privacy-indicator);
  * while location is off, QtPositioning's geoclue2 cache
    (~/.local/share/qtposition-geoclue2, replayed by Qt apps as a fresh
    position on startUpdates even when GeoClue refuses) is replaced by an
    empty directory, so Qt can neither replay nor re-save it; it is removed
    again when location is turned on.
"""
import json
import os
import shutil
import subprocess
import time

from gi.repository import Gio, GLib

BUS_NAME = "com.lomiri.indicator.location"
OBJ = "/com/lomiri/indicator/location"
UNIT = "fp6-gnss-nmea.service"
AGENT_ID = "fp6-location-indicator"
AGENT_PATH = "/org/freedesktop/GeoClue2/Agent"
AGENT_XML = """<node><interface name="org.freedesktop.GeoClue2.Agent">
<method name="AuthorizeApp"><arg name="desktop_id" type="s" direction="in"/>
<arg name="req_accuracy_level" type="u" direction="in"/>
<arg name="authorized" type="b" direction="out"/>
<arg name="allowed_accuracy_level" type="u" direction="out"/></method>
<property name="MaxAccuracyLevel" type="u" access="read"/>
</interface></node>"""
ACCURACY_EXACT = 8
PERMD = "org.fairphone.Permissions1"
PERMD_PATH = "/org/fairphone/Permissions1"
USE_PERMD = os.environ.get("FP6_LOCATION_PERMISSIONS") == "1"
CONF = os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"),
                    "fp6-location", "enabled")
QT_CACHE = os.path.join(os.environ.get("XDG_DATA_HOME") or os.path.expanduser("~/.local/share"),
                        "qtposition-geoclue2")
APPS_LOG = os.path.join(os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}",
                        "fp6-privacy", "location-apps.json")


def log(msg):
    print(f"fp6-location-indicator: {msg}", flush=True)


def load_enabled():
    try:
        with open(CONF) as f:
            return f.read().strip() == "1"
    except OSError:
        return False


def save_enabled(on):
    try:
        os.makedirs(os.path.dirname(CONF), exist_ok=True)
        with open(CONF, "w") as f:
            f.write("1\n" if on else "0\n")
    except OSError as e:
        log(f"cannot store setting: {e}")


def block_qt_cache(block):
    """Off: a directory where QtPositioning keeps its last position file."""
    try:
        if block:
            if os.path.isdir(QT_CACHE):
                return
            if os.path.lexists(QT_CACHE):
                os.remove(QT_CACHE)
                log("removed the cached Qt position")
            os.makedirs(QT_CACHE, mode=0o700)
        elif os.path.isdir(QT_CACHE) and not os.path.islink(QT_CACHE):
            shutil.rmtree(QT_CACHE, ignore_errors=True)
    except OSError as e:
        log(f"Qt position cache: {e}")


def record_app(app, granted, history=[]):
    history.append({"id": app, "time": time.time(), "granted": bool(granted)})
    del history[:-20]
    try:
        os.makedirs(os.path.dirname(APPS_LOG), exist_ok=True)
        tmp = APPS_LOG + ".tmp"
        with open(tmp, "w") as f:
            json.dump({"apps": history}, f)
        os.replace(tmp, APPS_LOG)
    except OSError as e:
        log(f"cannot record location app: {e}")


class Indicator:
    def __init__(self):
        self.system = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
        self.enabled = load_enabled()
        self.actions = Gio.SimpleActionGroup()
        self.root = Gio.SimpleAction.new_stateful("root-phone", None, self.root_state())
        self.actions.add_action(self.root)
        for name in ("location-detection-enabled", "gps-detection-enabled"):
            a = Gio.SimpleAction.new_stateful(name, None, GLib.Variant("b", self.enabled))
            a.connect("change-state", self.on_change_state)
            a.connect("activate", self.on_activate)
            self.actions.add_action(a)
        # Location.qml also probes these; GPS is the only source here.
        lic = Gio.SimpleAction.new_stateful("licence-accepted", None, GLib.Variant("b", True))
        self.actions.add_action(lic)
        settings = Gio.SimpleAction.new("phone-settings", None)
        settings.connect("activate", self.open_settings)
        self.actions.add_action(settings)

        sub = Gio.Menu()
        section = Gio.Menu()
        item = Gio.MenuItem.new("Location detection", "indicator.location-detection-enabled")
        item.set_attribute_value("x-ayatana-type", GLib.Variant("s", "org.ayatana.indicator.switch"))
        section.append_item(item)
        section.append_item(Gio.MenuItem.new("Location settings…", "indicator.phone-settings"))
        sub.append_section(None, section)
        root_item = Gio.MenuItem.new(None, "indicator.root-phone")
        root_item.set_attribute_value("x-ayatana-type", GLib.Variant("s", "org.ayatana.indicator.root"))
        root_item.set_submenu(sub)
        self.menu = Gio.Menu()
        self.menu.append_item(root_item)

    def root_state(self):
        icon = Gio.ThemedIcon.new_from_names(
            ["location-active", "gps", "location"] if self.enabled
            else ["location-disabled", "location-idle", "location"])
        return GLib.Variant("a{sv}", {
            "title": GLib.Variant("s", "Location"),
            "accessible-desc": GLib.Variant("s", "Location detection " + ("on" if self.enabled else "off")),
            "icon": icon.serialize(),
            # Like Ubuntu Touch: the status-bar icon only shows while enabled,
            # the indicator stays reachable from the panel either way.
            "visible": GLib.Variant("b", True),
        })

    def set_enabled(self, on):
        if on == self.enabled:
            return
        try:
            self.system.call_sync("org.freedesktop.systemd1", "/org/freedesktop/systemd1",
                                  "org.freedesktop.systemd1.Manager",
                                  "StartUnit" if on else "StopUnit",
                                  GLib.Variant("(ss)", (UNIT, "replace")),
                                  GLib.VariantType("(o)"), Gio.DBusCallFlags.NONE, 20000, None)
        except GLib.Error as e:
            log(f"{'start' if on else 'stop'} {UNIT} failed: {e.message}")
            return
        self.enabled = on
        save_enabled(on)
        block_qt_cache(not on)
        for name in ("location-detection-enabled", "gps-detection-enabled"):
            self.actions.lookup_action(name).set_state(GLib.Variant("b", on))
        self.root.set_state(self.root_state())
        self.agent_changed()
        log(f"location detection {'on' if on else 'off'}")

    # ---- GeoClue agent ------------------------------------------------
    def export_agent(self):
        node = Gio.DBusNodeInfo.new_for_xml(AGENT_XML)
        self.system.register_object(AGENT_PATH, node.interfaces[0],
                                    self.agent_call, self.agent_get, None)
        # (Re-)register whenever GeoClue (D-Bus activated, exits when idle)
        # appears; the agent must be known before any app asks.
        Gio.bus_watch_name_on_connection(self.system, "org.freedesktop.GeoClue2",
                                         Gio.BusNameWatcherFlags.AUTO_START,
                                         lambda *_: GLib.idle_add(self.register_agent), None)

    def register_agent(self):
        # Async: GeoClue reads our MaxAccuracyLevel while handling AddAgent,
        # so a blocking call here would deadlock until the D-Bus timeout.
        def done(conn, res):
            try:
                conn.call_finish(res)
                log("registered as GeoClue agent")
            except GLib.Error as e:
                log(f"GeoClue agent registration failed: {e.message}")
        self.system.call("org.freedesktop.GeoClue2", "/org/freedesktop/GeoClue2/Manager",
                         "org.freedesktop.GeoClue2.Manager", "AddAgent",
                         GLib.Variant("(s)", (AGENT_ID,)), None,
                         Gio.DBusCallFlags.NONE, 20000, None, done)
        return False

    def max_accuracy(self):
        return ACCURACY_EXACT if self.enabled else 0

    def from_geoclue(self, sender):
        try:
            owner = self.system.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus",
                                          "org.freedesktop.DBus", "GetNameOwner",
                                          GLib.Variant("(s)", ("org.freedesktop.GeoClue2",)),
                                          GLib.VariantType("(s)"), Gio.DBusCallFlags.NONE, 2000, None)
            return owner.unpack()[0] == sender
        except GLib.Error:
            return False

    def agent_call(self, conn, sender, path, iface, method, params, inv):
        if method != "AuthorizeApp":
            inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)
            return
        app, req = params.unpack()
        if USE_PERMD and not self.from_geoclue(sender):
            inv.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "only GeoClue may ask")
            return

        def answer(ok):
            record_app(app, ok)
            inv.return_value(GLib.Variant("(bu)", (ok, min(req, self.max_accuracy()) if ok else 0)))

        if not self.enabled or not USE_PERMD:
            answer(self.enabled)
            return

        def done(conn, res):
            try:
                ok = conn.call_finish(res).unpack()[0] == "granted"
            except GLib.Error as e:
                log(f"permission broker: {e.message}")
                ok = False
            answer(ok and self.enabled)

        self.system.call(PERMD, PERMD_PATH, PERMD, "AuthorizeApp",
                         GLib.Variant("(sss)", ("location", app.removesuffix(".desktop"), "")),
                         GLib.VariantType("(s)"), Gio.DBusCallFlags.NONE, 100000, None, done)

    def agent_get(self, conn, sender, path, iface, prop):
        if prop == "MaxAccuracyLevel":
            return GLib.Variant("u", self.max_accuracy())
        return None

    def agent_changed(self):
        try:
            self.system.emit_signal(None, AGENT_PATH, "org.freedesktop.DBus.Properties",
                                    "PropertiesChanged",
                                    GLib.Variant("(sa{sv}as)", ("org.freedesktop.GeoClue2.Agent",
                                                 {"MaxAccuracyLevel": GLib.Variant("u", self.max_accuracy())}, [])))
        except GLib.Error:
            pass

    def on_change_state(self, action, value):
        self.set_enabled(value.get_boolean())

    def on_activate(self, action, _param):
        self.set_enabled(not action.get_state().get_boolean())

    def open_settings(self, *_):
        subprocess.Popen(["lomiri-url-dispatcher", "settings:///system/location"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def sync_unit(self):
        # Apply the stored choice at login (unit is not enabled at boot).
        want = self.enabled
        self.enabled = not want
        self.set_enabled(want)
        if self.enabled != want:  # call failed; reflect reality (off)
            self.enabled = False
        block_qt_cache(not self.enabled)
        return False

    def on_bus(self, conn, _name):
        conn.export_action_group(OBJ, self.actions)
        conn.export_menu_model(OBJ + "/phone", self.menu)
        conn.export_menu_model(OBJ + "/desktop", self.menu)
        GLib.idle_add(self.sync_unit)
        self.export_agent()


def main():
    ind = Indicator()
    Gio.bus_own_name(Gio.BusType.SESSION, BUS_NAME, Gio.BusNameOwnerFlags.NONE,
                     ind.on_bus, None, lambda *_: loop.quit())
    loop = GLib.MainLoop()
    loop.run()


if __name__ == "__main__":
    main()
