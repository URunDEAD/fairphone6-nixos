"""Create one Telepathy account per real oFono modem for telepathy-ofono.

Replaces the Android-only mc-plugin/ofono-setup (getprop, /ril_N) that the
FP6 telepathy-ofono build deliberately omits. Runs in the user session:
  - reads org.ofono Manager.GetModems on the SYSTEM bus (no modem method is
    called; nothing is powered, onlined, unlocked or dialled),
  - on the SESSION bus, creates ofono/ofono/accountN with
    modem-objpath=<real path> only if no existing ofono account already
    references that path.
It never invents modems, never deletes accounts and never logs SIM/subscriber
identifiers (oFono object paths such as /embedded/qrtr/12 are not identifiers).
"""
import sys
import time

from gi.repository import Gio, GLib

AM_NAME = "org.freedesktop.Telepathy.AccountManager"
AM_PATH = "/org/freedesktop/Telepathy/AccountManager"
ACCOUNT_IFACE = "org.freedesktop.Telepathy.Account"
OFONO_PREFIX = "/org/freedesktop/Telepathy/Account/ofono/ofono/"
POLL_SECONDS = 20


def log(msg):
    print(f"fp6-ofono-account: {msg}", flush=True)


def call(bus, name, path, iface, method, params=None, reply=None, timeout=5000):
    return bus.call_sync(name, path, iface, method, params,
                         GLib.VariantType(reply) if reply else None,
                         Gio.DBusCallFlags.NONE, timeout, None).unpack()


def get_prop(bus, name, path, iface, prop):
    return call(bus, name, path, "org.freedesktop.DBus.Properties", "Get",
                GLib.Variant("(ss)", (iface, prop)), "(v)")[0]


def ofono_modems(system):
    try:
        modems = call(system, "org.ofono", "/", "org.ofono.Manager", "GetModems",
                      None, "(a(oa{sv}))")[0]
    except GLib.Error:
        return []
    # Only real modems (skip e.g. hfp_ or phonesim test modems).
    return [path for path, props in modems
            if props.get("Type", "hardware") == "hardware"]


def existing_accounts(session):
    """Map ofono account object path -> modem-objpath ('' if unset)."""
    accts = {}
    for kind in ("ValidAccounts", "InvalidAccounts"):
        try:
            accounts = get_prop(session, AM_NAME, AM_PATH, AM_NAME, kind)
        except GLib.Error:
            continue
        for acct in accounts:
            if not acct.startswith(OFONO_PREFIX):
                continue
            try:
                params = get_prop(session, AM_NAME, acct, ACCOUNT_IFACE, "Parameters")
            except GLib.Error:
                continue
            accts[acct] = params.get("modem-objpath", "")
    return accts


def repoint_account(session, acct, modem_path):
    """oFono numbers qrtrsoc modems per daemon run (/qrtrsoc_0, _1, ...), so
    after an oFono restart the account may point at a vanished path. Reuse it
    instead of piling up accounts (the Messages app then picks a dead one)."""
    call(session, AM_NAME, acct, ACCOUNT_IFACE, "UpdateParameters",
         GLib.Variant("(a{sv}as)", ({"modem-objpath": GLib.Variant("s", modem_path)}, [])),
         "(as)", timeout=20000)
    for prop, val in (("Enabled", GLib.Variant("b", True)),
                      ("RequestedPresence", GLib.Variant("(uss)", (2, "available", "")))):
        call(session, AM_NAME, acct, "org.freedesktop.DBus.Properties", "Set",
             GLib.Variant("(ssv)", (ACCOUNT_IFACE, prop, val)))
    try:
        call(session, AM_NAME, acct, ACCOUNT_IFACE, "Reconnect", timeout=20000)
    except GLib.Error:
        pass
    log(f"re-pointed {acct} to modem {modem_path}")


def create_account(session, modem_path, index):
    # Plain dicts: a GLib.Variant nested inside the tuple below breaks
    # PyGObject's builder (KeyError: 0), which killed the first version.
    params = {"modem-objpath": GLib.Variant("s", modem_path)}
    props = {
        ACCOUNT_IFACE + ".Enabled": GLib.Variant("b", True),
        ACCOUNT_IFACE + ".ConnectAutomatically": GLib.Variant("b", True),
        ACCOUNT_IFACE + ".RequestedPresence":
            GLib.Variant("(uss)", (2, "available", "")),
    }
    obj = call(session, AM_NAME, AM_PATH, AM_NAME, "CreateAccount",
               GLib.Variant("(sssa{sv}a{sv})",
                            ("ofono", "ofono", f"SIM {index}", params, props)),
               "(o)", timeout=20000)[0]
    log(f"created {obj} for modem {modem_path}")


def main():
    system = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    session = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    once = "--once" in sys.argv
    while True:
        try:
            modems = ofono_modems(system)
            if modems:
                accts = existing_accounts(session)
                known = set(accts.values())
                stale = sorted(a for a, p in accts.items() if p not in modems)
                for i, path in enumerate(sorted(modems), start=1):
                    if path in known:
                        continue
                    if stale:
                        repoint_account(session, stale.pop(0), path)
                    else:
                        create_account(session, path, i)
        except GLib.Error as e:
            log(f"transient D-Bus error: {e.message}")
        if once:
            return 0
        time.sleep(POLL_SECONDS)


if __name__ == "__main__":
    sys.exit(main())
