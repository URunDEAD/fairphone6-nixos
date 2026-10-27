from gi.repository import Gio, GLib
X = """<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg type="as" direction="out"/></method>
<method name="Notify"><arg type="s"/><arg type="u"/><arg type="s"/><arg type="s"/><arg type="s"/><arg type="as"/><arg type="a{sv}"/><arg type="i"/><arg type="u" direction="out"/></method>
<method name="CloseNotification"><arg type="u"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
</interface></node>"""
info = Gio.DBusNodeInfo.new_for_xml(X)
def call(conn, sender, path, iface, method, params, inv):
    if method == "GetCapabilities": inv.return_value(GLib.Variant("(as)", (["body", "x-lomiri-private-synchronous"],)))
    elif method == "Notify":
        print("NOTIFY", params.unpack()[3], params.unpack()[6].get("value"), flush=True); inv.return_value(GLib.Variant("(u)", (1,)))
    elif method == "GetServerInformation": inv.return_value(GLib.Variant("(ssss)", ("mock", "x", "1", "1.2")))
    else: inv.return_value(None)
def bus(conn, name):
    conn.register_object("/org/freedesktop/Notifications", info.interfaces[0], call, None, None)
Gio.bus_own_name(Gio.BusType.SESSION, "org.freedesktop.Notifications", 0, bus, None, None)
GLib.MainLoop().run()
