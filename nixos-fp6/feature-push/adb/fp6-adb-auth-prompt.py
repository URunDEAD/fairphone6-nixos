#!/usr/bin/env python3
"""FP6 "Allow USB debugging?" prompt for adbd (runs in the user's Lomiri session).

adbd (adbd-fp6-auth-user.patch) asks its "framework" socket /run/adbd/adbd
when a computer offers an RSA key that is not in /etc/adbd/adb_keys or
/var/lib/adbd/adb_keys: it writes "PK<base64 key> <comment>" and waits for
"OK" (accept this connection) or anything else (reject). This is the same
protocol Android's UsbDebuggingManager speaks.

The prompt is a Lomiri snap-decision notification with the key's MD5
fingerprint. It is only shown while the greeter (lock screen) is not
active; if the phone locks while it is up, it is withdrawn and the
connection is rejected. "Always allow" appends the key to
/var/lib/adbd/adb_keys. No answer within the timeout means reject.
"""
import base64
import hashlib
import os
import socket
import sys

import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

SOCKET = os.environ.get("FP6_ADB_AUTH_SOCKET", "/run/adbd/adbd")
KEYS_FILE = os.environ.get("FP6_ADB_KEYS_FILE", "/var/lib/adbd/adb_keys")
TIMEOUT_S = int(os.environ.get("FP6_ADB_PROMPT_TIMEOUT", "60"))
LOCK_WAIT_S = int(os.environ.get("FP6_ADB_LOCK_WAIT", "120"))


def log(msg):
    print(f"fp6-adb-auth-prompt: {msg}", flush=True)


def fingerprint(b64key):
    raw = base64.b64decode(b64key)
    digest = hashlib.md5(raw).hexdigest().upper()
    return ":".join(digest[i:i + 2] for i in range(0, len(digest), 2))


class Prompt:
    def __init__(self, bus, conn):
        self.bus = bus
        self.conn = conn
        self.notify = dbus.Interface(
            bus.get_object("org.freedesktop.Notifications", "/org/freedesktop/Notifications"),
            "org.freedesktop.Notifications")
        bus.add_signal_receiver(self.on_action, "ActionInvoked",
                                "org.freedesktop.Notifications")
        bus.add_signal_receiver(self.on_closed, "NotificationClosed",
                                "org.freedesktop.Notifications")
        self.pending = None   # (key line, notification id or None)
        self.gen = 0          # bumps on every finish(); stale timers stop

    # -- lock state -----------------------------------------------------------
    def locked(self):
        try:
            greeter = self.bus.get_object("com.lomiri.LomiriGreeter", "/com/lomiri/LomiriGreeter")
            return bool(greeter.Get("com.lomiri.LomiriGreeter", "IsActive",
                                    dbus_interface="org.freedesktop.DBus.Properties"))
        except dbus.exceptions.DBusException as e:
            # No greeter answer: treat as locked, never prompt blind.
            log(f"cannot read the lock state ({e.get_dbus_name()}); treating as locked")
            return True

    # -- adbd side ---------------------------------------------------------------
    def on_socket(self, _fd, condition):
        if condition & (GLib.IO_HUP | GLib.IO_ERR):
            log("adbd closed the authorization socket")
            loop.quit()
            return False
        data = self.conn.recv(8192)
        if not data:
            log("adbd closed the authorization socket")
            loop.quit()
            return False
        if data.startswith(b"PK"):
            key = data[2:].rstrip(b"\0").decode("ascii", "replace").strip()
            self.request(key)
        return True

    def reply(self, ok):
        try:
            self.conn.sendall(b"OK" if ok else b"NO")
        except OSError as e:
            log(f"reply failed: {e}")

    # -- prompt ------------------------------------------------------------------
    def finish(self, ok, store=False):
        if not self.pending:
            return
        key, nid = self.pending
        self.pending = None
        self.gen += 1
        if nid:
            try:
                self.notify.CloseNotification(dbus.UInt32(nid))
            except dbus.exceptions.DBusException:
                pass
        if ok and store:
            os.makedirs(os.path.dirname(KEYS_FILE), exist_ok=True)
            fd = os.open(KEYS_FILE, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
            with os.fdopen(fd, "a") as f:
                f.write(key + "\n")
            log(f"key {fingerprint(key.split()[0])} stored in {KEYS_FILE}")
        log("accepted" if ok else "rejected")
        self.reply(ok)

    def request(self, key):
        if self.pending:
            self.finish(False)
        parts = key.split(None, 1)
        try:
            fp = fingerprint(parts[0])
        except Exception:
            log("malformed key, rejected")
            self.reply(False)
            return
        comment = parts[1] if len(parts) > 1 else "unknown computer"
        log(f"key {fp} ({comment}) asks for USB debugging")
        self.pending = (key, None)
        self.waited = 0
        if self.locked():
            log("phone is locked; waiting for unlock before asking")
            GLib.timeout_add_seconds(1, self.wait_unlock, self.gen, fp, comment)
        else:
            self.show(fp, comment)

    def wait_unlock(self, gen, fp, comment):
        if gen != self.gen or not self.pending:
            return False
        self.waited += 1
        if not self.locked():
            self.show(fp, comment)
            return False
        if self.waited >= LOCK_WAIT_S:
            log("still locked; rejecting (reconnect USB to ask again)")
            self.finish(False)
            return False
        return True

    def show(self, fp, comment):
        key, _ = self.pending
        hints = {
            "x-lomiri-snap-decisions": dbus.Boolean(True, variant_level=1),
            "x-lomiri-snap-decisions-timeout": dbus.Int32(TIMEOUT_S * 1000, variant_level=1),
            "x-lomiri-private-affirmative-tint": dbus.String("true", variant_level=1),
            "urgency": dbus.Byte(2, variant_level=1),
        }
        actions = ["always", "Always allow", "deny", "Deny", "once", "Allow once"]
        body = (f"Allow USB debugging from “{comment}”?\n"
                f"RSA key fingerprint:\n{fp}\n"
                "It gets a shell as this user on the phone.")
        nid = self.notify.Notify("Developer mode", dbus.UInt32(0), "computer-symbolic",
                                 "Allow USB debugging?", body, actions,
                                 dbus.Dictionary(hints, signature="sv"), dbus.Int32(TIMEOUT_S * 1000))
        self.pending = (key, int(nid))
        GLib.timeout_add_seconds(TIMEOUT_S, self.on_timeout, self.gen)
        GLib.timeout_add_seconds(1, self.watch_lock, self.gen)

    def on_timeout(self, gen):
        if gen == self.gen and self.pending:
            log("no answer")
            self.finish(False)
        return False

    def watch_lock(self, gen):
        if gen != self.gen or not self.pending:
            return False
        if self.locked():
            log("phone locked while asking; withdrawn")
            self.finish(False)
            return False
        return True

    def on_action(self, nid, action):
        if not self.pending or self.pending[1] != int(nid):
            return
        if action == "always":
            self.finish(True, store=True)
        elif action == "once":
            self.finish(True)
        else:
            self.finish(False)

    def on_closed(self, nid, _reason):
        if self.pending and self.pending[1] == int(nid):
            self.finish(False)


if __name__ == "__main__":
    DBusGMainLoop(set_as_default=True)
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        conn.connect(SOCKET)
    except OSError as e:
        log(f"cannot connect to {SOCKET}: {e}")
        sys.exit(0)
    log(f"connected to {SOCKET}")
    loop = GLib.MainLoop()
    prompt = Prompt(dbus.SessionBus(), conn)
    GLib.io_add_watch(conn.fileno(), GLib.IO_IN | GLib.IO_HUP | GLib.IO_ERR, prompt.on_socket)
    loop.run()
    if prompt.pending:
        prompt.finish(False)
    # Do not let the path unit retrigger while adbd is going away.
    for _ in range(100):
        if not os.path.exists(SOCKET):
            break
        GLib.usleep(100000)
    sys.exit(0)
