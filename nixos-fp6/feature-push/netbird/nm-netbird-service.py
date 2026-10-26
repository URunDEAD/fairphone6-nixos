"""NetworkManager VPN plugin "netbird" for FP6 (org.freedesktop.NetworkManager.netbird).

Lets NetworkManager (and so lomiri-indicator-network's VPN switch, which lists
NM connections of type "vpn") turn the NetBird daemon on and off. NetBird keeps
managing its own interface (wt0, NM-unmanaged), addresses, routes and DNS
(systemd-resolved); this plugin only drives the daemon:

  Connect     -> `netbird up --profile <active> --disable-auto-connect`
                 (as the user that owns the daemon's active profile)
  Disconnect  -> `netbird down`

It reports an empty IP config to NM (has-ip4/has-ip6 false, no tun device),
so NM adds no addresses, routes or DNS. While active it polls `netbird status`
and reports a failure to NM if NetBird drops (e.g. `netbird down` on the CLI),
so the switch follows the real state.

No login happens here: if the active profile needs a login (setup key or SSO),
Connect fails with a log line pointing at `netbird login` / `netbird up` in a
terminal. NM spawns this program as root on demand; it exits after stopping.
"""
import json
import os
import pwd
import socket
import struct
import subprocess
import sys
import syslog
import threading
import warnings

from gi.repository import Gio, GLib

# Gio.DBusConnection.register_object (closure-less form) is fine here.
warnings.filterwarnings("ignore", category=DeprecationWarning)

# Overridable only for tests (a mock service under the same D-Bus policy prefix).
BUS_NAME = os.environ.get("FP6_NM_NETBIRD_BUS_NAME", "org.freedesktop.NetworkManager.netbird")
OBJ = "/org/freedesktop/NetworkManager/VPN/Plugin"
IFACE = "org.freedesktop.NetworkManager.VPN.Plugin"
NETBIRD = os.environ.get("FP6_NETBIRD_BIN", "netbird")
SETPRIV = os.environ.get("FP6_SETPRIV_BIN", "setpriv")
STATE_DIR = os.environ.get("NB_STATE_DIR", "/var/lib/netbird")
DAEMON_ADDR = os.environ.get("NB_DAEMON_ADDR", "unix:///var/run/netbird/sock")
CLI_STATE = os.environ.get("FP6_NM_NETBIRD_CLI_STATE", "/run/nm-netbird")

# NMVpnServiceState
ST_INIT, ST_SHUTDOWN, ST_STARTING, ST_STARTED, ST_STOPPING, ST_STOPPED = 1, 2, 3, 4, 5, 6
# NMVpnPluginFailure
FAIL_LOGIN, FAIL_CONNECT, FAIL_BAD_IP_CONFIG = 0, 1, 2

UP_TIMEOUT = 90
POLL_SECONDS = 10
NEEDS_LOGIN = ("NeedsLogin", "LoginFailed", "SessionExpired")

XML = """<node><interface name="org.freedesktop.NetworkManager.VPN.Plugin">
<method name="Connect"><arg name="connection" type="a{sa{sv}}" direction="in"/></method>
<method name="ConnectInteractive"><arg name="connection" type="a{sa{sv}}" direction="in"/>
<arg name="details" type="a{sv}" direction="in"/></method>
<method name="NeedSecrets"><arg name="settings" type="a{sa{sv}}" direction="in"/>
<arg name="setting_name" type="s" direction="out"/></method>
<method name="Disconnect"/>
<method name="SetConfig"><arg name="config" type="a{sv}" direction="in"/></method>
<method name="SetIp4Config"><arg name="config" type="a{sv}" direction="in"/></method>
<method name="SetIp6Config"><arg name="config" type="a{sv}" direction="in"/></method>
<method name="SetFailure"><arg name="reason" type="s" direction="in"/></method>
<method name="NewSecrets"><arg name="connection" type="a{sa{sv}}" direction="in"/></method>
<signal name="StateChanged"><arg name="state" type="u"/></signal>
<signal name="SecretsRequired"><arg name="message" type="s"/><arg name="secrets" type="as"/></signal>
<signal name="Config"><arg name="config" type="a{sv}"/></signal>
<signal name="Ip4Config"><arg name="ip4config" type="a{sv}"/></signal>
<signal name="Ip6Config"><arg name="ip6config" type="a{sv}"/></signal>
<signal name="LoginBanner"><arg name="banner" type="s"/></signal>
<signal name="Failure"><arg name="reason" type="u"/></signal>
<property name="State" type="u" access="read"/>
</interface></node>"""


def log(msg, prio=syslog.LOG_INFO):
    if os.environ.get("NM_VPN_LOG_SYSLOG") == "1":
        syslog.syslog(prio, msg)
    else:
        print(f"nm-netbird: {msg}", file=sys.stderr, flush=True)


def active_profile():
    """(profile id, username) of the daemon's active profile."""
    try:
        with open(os.path.join(STATE_DIR, "active_profile.json")) as f:
            st = json.load(f)
        return (st.get("name") or "default", st.get("username") or "")
    except (OSError, ValueError):
        return ("default", "")


def command(args, user=""):
    env = {"PATH": os.environ.get("PATH", "/run/current-system/sw/bin"),
           "NB_DAEMON_ADDR": DAEMON_ADDR, "LANG": "C.UTF-8"}
    cmd = [NETBIRD] + args
    if user and user != "root":
        try:
            pw = pwd.getpwnam(user)
        except KeyError:
            raise RuntimeError(f"profile owner {user!r} does not exist")
        # The profile lives under /var/lib/netbird/<user>/ and the CLI keeps
        # its active-profile pointer in ~/.config/netbird: run it as that user.
        cmd = [SETPRIV, f"--reuid={pw.pw_uid}", f"--regid={pw.pw_gid}", "--init-groups",
               "--"] + cmd
        # NetworkManager.service has ProtectHome=read-only, so the CLI's own
        # state (~/.config/netbird/active_profile.txt, rewritten by --profile)
        # goes to a scratch config dir; the daemon's active profile is the
        # source of truth anyway.
        # NM's capability set has no CAP_CHOWN: the CLI (as the user) creates
        # its own subdirectory in a sticky, world-writable but unlistable dir.
        os.makedirs(CLI_STATE, exist_ok=True)
        os.chmod(CLI_STATE, 0o1733)
        cfg = os.path.join(CLI_STATE, str(pw.pw_uid))
        try:
            st = os.lstat(cfg)
            if st.st_uid != pw.pw_uid or not os.path.isdir(cfg) or os.path.islink(cfg):
                raise RuntimeError(f"{cfg} is not a directory owned by {user}")
        except FileNotFoundError:
            pass
        env.update(HOME=pw.pw_dir, USER=user, LOGNAME=user, XDG_CONFIG_HOME=cfg)
    else:
        os.makedirs(os.path.join(CLI_STATE, "0"), mode=0o700, exist_ok=True)
        os.chmod(CLI_STATE, 0o1733)
        env.update(HOME="/root", USER="root", LOGNAME="root",
                   XDG_CONFIG_HOME=os.path.join(CLI_STATE, "0"))
    return cmd, env


def run(args, user="", timeout=30):
    cmd, env = command(args, user)
    p = subprocess.run(cmd, env=env, stdin=subprocess.DEVNULL, capture_output=True,
                       text=True, timeout=timeout)
    return p.returncode, (p.stdout + p.stderr).strip()


def run_up(prof, user):
    """`netbird up` for the profile. Returns (rc, output, needs_login).

    A profile that was never logged in looks "Idle" like a logged-in one, so
    the login need only shows when `up` asks for an SSO login: it then prints
    the verification URL and waits. That is stopped at once (no interactive
    login from NM); the URL goes to the log.
    """
    cmd, env = command(["up", "--profile", prof, "--disable-auto-connect", "--no-browser"], user)
    p = subprocess.Popen(cmd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, text=True)
    timer = threading.Timer(UP_TIMEOUT, p.kill)
    timer.start()
    lines, needs_login = [], False
    try:
        for line in p.stdout:
            lines.append(line.rstrip())
            if "to log in" in line or "SSO login" in line:
                needs_login = True
            if needs_login and "http" in line:
                p.terminate()
                break
        rc = p.wait()
    finally:
        timer.cancel()
        if p.poll() is None:
            p.kill()
            p.wait()
    if rc in (-9,) and not needs_login:
        raise subprocess.TimeoutExpired(cmd, UP_TIMEOUT)
    return rc, "\n".join(lines).strip(), needs_login


def status():
    rc, out = run(["status", "--json"], timeout=15)
    if rc != 0:
        raise RuntimeError(out or f"netbird status exited {rc}")
    return json.loads(out)


def daemon_status(st):
    return st.get("daemonStatus") or ""


class Plugin:
    def __init__(self, loop):
        self.loop = loop
        self.conn = None
        self.state = ST_INIT
        self.active = False
        self.poll_id = 0
        self.misses = 0
        self.quit_id = 0

    # ---- D-Bus plumbing ----------------------------------------------
    def emit(self, name, sig, *args):
        self.conn.emit_signal(None, OBJ, IFACE, name, GLib.Variant(sig, args))

    def set_state(self, st):
        if st == self.state:
            return
        self.state = st
        self.emit("StateChanged", "(u)", st)
        self.conn.emit_signal(None, OBJ, "org.freedesktop.DBus.Properties", "PropertiesChanged",
                              GLib.Variant("(sa{sv}as)", (IFACE, {"State": GLib.Variant("u", st)}, [])))
        if st == ST_STOPPED:
            self.schedule_quit()

    def schedule_quit(self):
        if not self.quit_id:
            self.quit_id = GLib.timeout_add_seconds(3, self.maybe_quit)

    def maybe_quit(self):
        self.quit_id = 0
        if self.state in (ST_STOPPED, ST_INIT):
            self.loop.quit()
        return False

    def fail(self, reason, msg):
        log(msg, syslog.LOG_WARNING)
        self.stop_poll()
        self.active = False
        self.emit("Failure", "(u)", reason)
        self.set_state(ST_STOPPED)

    def on_call(self, conn, sender, path, iface, method, params, inv):
        if method in ("Connect", "ConnectInteractive"):
            if self.state in (ST_STARTING, ST_STARTED):
                inv.return_dbus_error("org.freedesktop.NetworkManager.VPN.Error.AlreadyStarted",
                                      "NetBird VPN is already active")
                return
            if self.quit_id:
                GLib.source_remove(self.quit_id)
                self.quit_id = 0
            inv.return_value(None)
            self.set_state(ST_STARTING)
            threading.Thread(target=self.connect_worker, daemon=True).start()
        elif method == "NeedSecrets":
            inv.return_value(GLib.Variant("(s)", ("",)))  # nothing secret
        elif method == "Disconnect":
            inv.return_value(None)
            self.disconnect()
        elif method in ("SetConfig", "SetIp4Config", "SetIp6Config", "NewSecrets"):
            inv.return_value(None)
        elif method == "SetFailure":
            inv.return_value(None)
            self.fail(FAIL_CONNECT, "failure set: " + params.unpack()[0])
        else:
            inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)

    def on_get(self, conn, sender, path, iface, prop):
        if prop == "State":
            return GLib.Variant("u", self.state)
        return None

    # ---- connect / disconnect ----------------------------------------
    def connect_worker(self):
        try:
            prof, user = active_profile()
            st = status()
            ds = daemon_status(st)
            if ds in NEEDS_LOGIN:
                GLib.idle_add(self.fail, FAIL_LOGIN,
                              f"NetBird profile {prof} is not logged in ({ds}); run "
                              "`netbird login` (setup key or SSO) in a terminal first")
                return
            if ds != "Connected":
                rc, out, needs_login = run_up(prof, user)
                if needs_login:
                    GLib.idle_add(self.fail, FAIL_LOGIN,
                                  f"NetBird profile {prof} needs a login; run `netbird login` "
                                  f"(or `netbird up`) in a terminal. CLI said: {out}")
                    return
                if rc != 0:
                    GLib.idle_add(self.fail, FAIL_CONNECT, f"netbird up failed ({rc}): {out}")
                    return
                st = status()
            GLib.idle_add(self.connected, st, prof, user)
        except subprocess.TimeoutExpired:
            GLib.idle_add(self.fail, FAIL_CONNECT, "netbird up timed out (needs a login?)")
        except Exception as e:  # noqa: BLE001 - report every error to NM
            GLib.idle_add(self.fail, FAIL_CONNECT, f"connect failed: {e}")

    def connected(self, st, prof, user):
        if self.state != ST_STARTING:  # disconnected meanwhile
            return False
        ip = (st.get("netbirdIp") or "").split("/")[0]
        try:
            gw = struct.unpack("=I", socket.inet_aton(ip))[0]
        except OSError:
            gw = struct.unpack("=I", socket.inet_aton("127.0.0.1"))[0]
        banner = f"NetBird {ip} {st.get('fqdn') or ''}".strip()
        # NetBird configures wt0 itself: no tun device, no IPv4/IPv6 config
        # for NM. The external gateway is mandatory in the D-Bus API; the
        # connection sets auto-route-ext-gw=false so NM adds no route for it.
        self.emit("Config", "(a{sv})", {
            "tundev": GLib.Variant("s", ""),
            "gateway": GLib.Variant("u", gw),
            "has-ip4": GLib.Variant("b", False),
            "has-ip6": GLib.Variant("b", False),
            "can-persist": GLib.Variant("b", True),
            "banner": GLib.Variant("s", banner),
        })
        self.active = True
        self.set_state(ST_STARTED)
        log(f"connected: profile {prof} ({user or 'root'}), {banner}")
        self.misses = 0
        self.poll_id = GLib.timeout_add_seconds(POLL_SECONDS, self.poll)
        return False

    def poll(self):
        def worker():
            try:
                ds = daemon_status(status())
            except Exception as e:  # noqa: BLE001
                ds = f"error: {e}"
            GLib.idle_add(self.poll_result, ds)
        threading.Thread(target=worker, daemon=True).start()
        return True

    def poll_result(self, ds):
        if not self.active:
            return False
        if ds in ("Connected", "Connecting"):
            self.misses = 0
        else:
            self.misses += 1
            if self.misses >= 2:
                self.fail(FAIL_CONNECT, f"NetBird is no longer connected ({ds})")
        return False

    def stop_poll(self):
        if self.poll_id:
            GLib.source_remove(self.poll_id)
            self.poll_id = 0

    def disconnect(self):
        self.stop_poll()
        self.active = False
        self.set_state(ST_STOPPING)

        def worker():
            try:
                rc, out = run(["down"], timeout=30)
                if rc != 0:
                    log(f"netbird down failed ({rc}): {out}", syslog.LOG_WARNING)
            except Exception as e:  # noqa: BLE001
                log(f"netbird down failed: {e}", syslog.LOG_WARNING)
            GLib.idle_add(self.set_state, ST_STOPPED)
        threading.Thread(target=worker, daemon=True).start()

    # ---- bus ----------------------------------------------------------
    def on_bus(self, conn, _name):
        self.conn = conn
        node = Gio.DBusNodeInfo.new_for_xml(XML)
        conn.register_object(OBJ, node.interfaces[0], self.on_call, self.on_get, None)

    def on_name(self, conn, _name):
        log("service started")
        # NM connects within seconds of spawning us; exit when unused.
        GLib.timeout_add_seconds(60, lambda: (self.maybe_quit() if not self.active and self.state != ST_STARTING else False))

    def on_lost(self, conn, _name):
        log("lost bus name, exiting", syslog.LOG_WARNING)
        self.loop.quit()


def main():
    syslog.openlog("nm-netbird-service", syslog.LOG_PID, syslog.LOG_DAEMON)
    bus_name = BUS_NAME
    if "--bus-name" in sys.argv:
        bus_name = sys.argv[sys.argv.index("--bus-name") + 1]
    loop = GLib.MainLoop()
    plugin = Plugin(loop)
    Gio.bus_own_name(Gio.BusType.SYSTEM, bus_name, Gio.BusNameOwnerFlags.NONE,
                     plugin.on_bus, plugin.on_name, plugin.on_lost)
    loop.run()
    if plugin.active:
        run(["down"], timeout=30)


if __name__ == "__main__":
    main()
