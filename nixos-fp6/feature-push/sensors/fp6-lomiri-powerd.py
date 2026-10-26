#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""FP6 repowerd-compatible display backend for Lomiri.

Supersedes nixos-fp6/fp6-brightness-dbus.py (same manual-brightness contract
and caller authorisation, copied here unchanged in behaviour) and adds:

  * automatic brightness from net.hadess.SensorProxy LightLevel
    (com.canonical.Unity.Screen / com.lomiri.Repowerd userAutobrightnessEnable,
    advertised through getBrightnessParams()[4])
  * call proximity blanking (com.lomiri.Repowerd enable/disableProximityHandling,
    used by lomiri-telephony-service's handler)
  * double-tap-to-wake (Unity.Screen getDoubleTapToWakeSupported /
    setDoubleTapToWakeEnabled) on top of the ESWIN gesture_wakeup + lp_mode
    sysfs attributes added by kernel-build/feature-push/sensors/0003
  * optional screen power (Unity.Screen setScreenPowerMode, emitted as
    DisplayPowerStateChange) behind --screen-power, emulated with backlight 0
    plus touch gesture-LP; this backend never suspends the system.
  * brightness cap (com.fairphone6.BrightnessCap SetCap/GetCap/CapChanged on
    /com/fairphone6/BrightnessCap) for battery saver: every write is clamped,
    the slider/auto target is kept and comes back when the cap is lifted.
  * idle display-off (with --screen-power): Lomiri's setInactivityTimeouts
    (Settings > Lock when idle = com.lomiri.touch.system activity-timeout,
    dim-timeout): dim, then display off with reason "inactivity" (Lomiri
    then locks). Held off by keepDisplayOn requests (freed when their caller
    leaves the bus) and by proximity handling (calls).
  * com.fairphone6.PowerKey.Press: one signal per short power press (the
    privacy-lock SOS counter), independent of the multi-press mode.
  * com.fairphone6.Gestures (fp6-gestures): pocket mode (SetPocket: while
    covered, power key / double tap / notification wakes are refused after a
    fresh proximity read, touch stays in LP, a lit display goes off after
    5 s; incoming calls still wake; a second power press within 3 s wakes
    anyway) and raise-to-wake (Peek: display on, off again if no input).

Nothing here suspends or touches the IMU.
"""
import argparse
import math
import os
from pathlib import Path
import signal
import struct
import sys
import threading
import time
import xml.etree.ElementTree as ET

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

REPOWERD = "com.lomiri.Repowerd"
SCREEN = "com.canonical.Unity.Screen"
REPOWERD_PATH = "/com/lomiri/Repowerd"
SCREEN_PATH = "/com/canonical/Unity/Screen"
PROPERTIES = "org.freedesktop.DBus.Properties"
INTROSPECTABLE = "org.freedesktop.DBus.Introspectable"
LOGIN = "org.freedesktop.login1"
LOGIN_PATH = "/org/freedesktop/login1"
SENSORS = "net.hadess.SensorProxy"
SENSORS_PATH = "/net/hadess/SensorProxy"
LOGICAL_MAX = 255

# Lomiri plugins/Powerd/Powerd.h DisplayStateChangeReason
REASON_UNKNOWN, REASON_INACTIVITY, REASON_POWERKEY, REASON_PROXIMITY = 0, 1, 2, 3
REASON_NOTIFICATION, REASON_SNAP_DECISION = 4, 5
NOTIFICATION_REASONS = (REASON_NOTIFICATION, REASON_SNAP_DECISION)
EV_ABS = 0x03

# Default lux -> logical (1..255) curve, interpolated in log10(1 + lux).
# The STK36C61 lux scale is the legacy STK3310 table (uncalibrated), so this is
# deliberately conservative and overridable with --curve.
DEFAULT_CURVE = ((0, 12), (5, 30), (20, 55), (80, 90), (300, 135), (1000, 180), (5000, 235), (20000, 255))

TOUCH_NAME = "ESWIN EPH86XX Touchscreen"
POWER_KEY_NAME = "pmic_pwrkey"
EV_KEY, KEY_WAKEUP, KEY_POWER = 0x01, 143, 116
# Power + Volume-Down is Lomiri's screenshot chord (screen-capture module):
# a power press that overlaps a Volume-Down press leaves the display alone.
KEY_VOLUMEDOWN = 114
# Shorter presses toggle the display (repowerd behaviour); longer ones are left
# to Lomiri's own long-press power dialog (PhysicalKeysMapper, 2 s).
POWER_KEY_SHORT_PRESS = 1.0
# Double/triple press: the next press must start within this time after the
# previous release (only waited for when a multi-press action is assigned).
POWER_KEY_MULTI_GAP = 0.35
POWERKEY_IFACE = "com.fairphone6.PowerKey"
POWERKEY_PATH = "/com/fairphone6/PowerKey"
# Idle dimming: backlight level (logical) while dimmed before idle display-off.
IDLE_DIM_VALUE = 10
# Brightness cap (battery saver, fp6-batteryd): exported on the
# com.lomiri.Repowerd connection, so the existing bus policy applies.
CAP_IFACE = "com.fairphone6.BrightnessCap"
CAP_PATH = "/com/fairphone6/BrightnessCap"
# Pocket mode and raise-to-wake for the gestures daemon (fp6-gestures).
GESTURES_IFACE = "com.fairphone6.Gestures"
GESTURES_PATH = "/com/fairphone6/Gestures"
POCKET_OVERRIDE_WINDOW = 3.0   # second blocked power press within this wakes anyway
POCKET_LATE_RELEASE = 0.6      # uncovered this soon after a blocked press: wake
POCKET_DISPLAY_OFF = 5.0       # a lit display that gets pocketed goes off after this
PROXIMITY_SENSOR = "stk36c61"
EVENT = struct.Struct("llHHi")


def log(*args):
    print(*args, flush=True)


class Denied(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.AccessDenied"


class Invalid(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.InvalidArgs"


class Unsupported(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.NotSupported"


class Failed(dbus.DBusException):
    _dbus_error_name = "org.freedesktop.DBus.Error.Failed"


# ---------------------------------------------------------------- policy math
def parse_curve(text):
    points = []
    for item in text.split(","):
        lux, _, value = item.partition(":")
        points.append((float(lux), int(value)))
    points.sort()
    if len(points) < 2 or any(not 1 <= v <= LOGICAL_MAX for _, v in points) or points[0][0] < 0:
        raise ValueError("curve needs >=2 'lux:value' points with 1<=value<=255")
    return tuple(points)


def curve_value(curve, lux):
    """Piecewise-linear in log10(1+lux); clamps outside the table."""
    x = math.log10(1.0 + max(0.0, float(lux)))
    pts = [(math.log10(1.0 + p), v) for p, v in curve]
    if x <= pts[0][0]:
        return pts[0][1]
    for (x0, v0), (x1, v1) in zip(pts, pts[1:]):
        if x <= x1:
            return int(round(v0 + (v1 - v0) * (x - x0) / (x1 - x0)))
    return pts[-1][1]


class LuxFilter:
    """Asymmetric EMA: brighten fast (sunlight), dim slowly (shadows)."""

    def __init__(self, rise=0.5, fall=0.15):
        self.rise, self.fall, self.value = rise, fall, None

    def push(self, lux):
        lux = max(0.0, float(lux))
        if self.value is None:
            self.value = lux
        else:
            alpha = self.rise if lux > self.value else self.fall
            self.value += alpha * (lux - self.value)
        return self.value


def needs_change(current, target, rel=0.08, absolute=3):
    return abs(target - current) > max(absolute, rel * current)


# ------------------------------------------------------------------- hardware
class Backlight:
    def __init__(self, directory=Path("/sys/class/backlight/ae94000.dsi.0"),
                 compatible=Path("/proc/device-tree/compatible"), expect_max=4095):
        self.directory = Path(directory)
        self.lock = threading.Lock()
        self.cap = LOGICAL_MAX       # com.fairphone6.BrightnessCap (logical)
        if compatible is not None and b"fairphone,fp6" not in Path(compatible).read_bytes().split(b"\0"):
            raise RuntimeError("This backend requires fairphone,fp6")
        self.maximum = self._integer("max_brightness")
        if expect_max is not None and (self.maximum != expect_max or self._text("type") != "raw"):
            raise RuntimeError("Unexpected FP6 backlight type or maximum")
        self.initial = self.read()

    def _text(self, name):
        return (self.directory / name).read_text(encoding="ascii").strip()

    def _integer(self, name):
        text = self._text(name)
        if not text.isdecimal():
            raise RuntimeError("Invalid backlight metadata")
        return int(text)

    def raw(self):
        with self.lock:
            return self._integer("brightness")

    def read(self):
        raw = self.raw()
        if not 0 <= raw <= self.maximum:
            raise RuntimeError("Backlight readback is outside hardware bounds")
        return (raw * LOGICAL_MAX + self.maximum // 2) // self.maximum

    def _write_raw(self, raw):
        fd = os.open(self.directory / "brightness", os.O_WRONLY | os.O_TRUNC | os.O_CLOEXEC | os.O_NOFOLLOW)
        try:
            payload = f"{raw}\n".encode("ascii")
            if os.write(fd, payload) != len(payload):
                raise Failed("Short backlight write")
        finally:
            os.close(fd)
        if self._integer("brightness") != raw:
            raise Failed("Backlight write did not match sysfs readback")

    def write(self, value):
        value = int(value)
        if not 1 <= value <= LOGICAL_MAX:
            raise Invalid("Brightness must be between 1 and 255")
        value = min(value, self.cap)
        with self.lock:
            if self._integer("max_brightness") != self.maximum:
                raise Failed("Backlight maximum changed")
            raw = (value * self.maximum + LOGICAL_MAX // 2) // LOGICAL_MAX
            self._write_raw(raw)
            return (raw * LOGICAL_MAX + self.maximum // 2) // self.maximum

    def blank(self):
        """Display-off emulation: DCS brightness 0 (panel stays powered)."""
        with self.lock:
            self._write_raw(0)


class Touch:
    """ESWIN gesture sysfs (patched kernel) + KEY_WAKEUP evdev reader."""

    def __init__(self, sysfs=Path("/sys/bus/spi/devices/spi0.0"), input_root=Path("/sys/class/input")):
        self.sysfs = Path(sysfs)
        self.input_root = Path(input_root)

    @property
    def supported(self):
        return (self.sysfs / "gesture_wakeup").exists() and (self.sysfs / "lp_mode").exists()

    def _write(self, name, text):
        fd = os.open(self.sysfs / name, os.O_WRONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
        try:
            os.write(fd, text.encode("ascii"))
        finally:
            os.close(fd)

    def gesture_enabled(self):
        try:
            return (self.sysfs / "gesture_wakeup").read_text().startswith("enable")
        except OSError:
            return False

    def set_double_tap(self, enabled):
        # Driver ABI: 2 selects BIT2 (double tap), -2 clears it.
        self._write("gesture_wakeup", "2" if enabled else "-2")

    def set_lp(self, enabled):
        self._write("lp_mode", "1" if enabled else "0")

    def event_node(self):
        for entry in sorted(self.input_root.glob("event*")):
            try:
                if (entry / "device" / "name").read_text().strip() == TOUCH_NAME:
                    return Path("/dev/input") / entry.name
            except OSError:
                continue
        return None


# ------------------------------------------------------------- authorisation
class Authorizer:
    """Same rules as fp6-brightness-dbus.py: root, or the phone user's active
    local seat0 session / its systemd user manager while that session is active."""

    def __init__(self, bus, uid, allow_any=False):
        self.bus, self.uid, self.allow_any = bus, uid, allow_any
        self.daemon = dbus.Interface(bus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"),
                                     "org.freedesktop.DBus")
        self.manager = None if allow_any else dbus.Interface(
            bus.get_object(LOGIN, LOGIN_PATH, introspect=False), LOGIN + ".Manager")

    def _properties(self, path, interface):
        return dbus.Interface(self.bus.get_object(LOGIN, path, introspect=False), PROPERTIES).GetAll(
            interface, timeout=3)

    @staticmethod
    def _is_local_active(properties, uid):
        return (bool(properties.get("Active", False))
                and not bool(properties.get("Remote", True))
                and str(properties.get("Seat", ("",))[0]) == "seat0"
                and int(properties.get("User", (-1,))[0]) == uid)

    @staticmethod
    def _is_user_manager_pid(pid, uid):
        cgroup = Path(f"/proc/{pid}/cgroup").read_text()
        components = f"/user.slice/user-{uid}.slice/user@{uid}.service/"
        return any(components in line for line in cgroup.splitlines())

    def require(self, sender):
        if self.allow_any:
            return
        try:
            uid = int(self.daemon.GetConnectionUnixUser(sender, timeout=3))
            if uid == 0:
                return
            if uid != self.uid:
                raise Denied("Caller is not the configured phone user")
            pid = int(self.daemon.GetConnectionUnixProcessID(sender, timeout=3))
            try:
                path = self.manager.GetSessionByPID(dbus.UInt32(pid), timeout=3)
                if self._is_local_active(self._properties(path, LOGIN + ".Session"), uid):
                    return
                raise Denied("Caller session is not active and local on seat0")
            except dbus.DBusException as error:
                if error.get_dbus_name() != LOGIN + ".NoSessionForPID":
                    raise
            if not self._is_user_manager_pid(pid, uid):
                raise Denied("Caller is outside the phone user manager")
            seat = self._properties("/org/freedesktop/login1/seat/seat0", LOGIN + ".Seat")
            path = seat["ActiveSession"][1]
            if self._is_local_active(self._properties(path, LOGIN + ".Session"), uid):
                return
            raise Denied("Phone user has no active local seat0 session")
        except Denied:
            raise
        except Exception:
            raise Denied("Could not verify caller's active local session") from None


# ------------------------------------------------------------------ the core
class Core:
    """Display state machine shared by both D-Bus objects."""

    volume_down_held = False      # Volume-Down currently pressed
    power_chord = False           # this power press overlaps Volume-Down
    multi_press_max = 0           # 0, 2 or 3 (com.fairphone6.PowerKey.SetMultiPress)
    multi_count = 0
    multi_timer = None
    multi_listeners = ()
    press_listeners = ()          # callables(monotonic time, display on): every short press
    screen_power = False
    # idle display-off (Unity.Screen setInactivityTimeouts), 0 = never
    idle_timeout = 0
    dim_timeout = 0
    idle_timer = None
    idle_dimmed = False
    last_activity = 0.0
    display_on_requests = None    # cookie -> D-Bus sender (keepDisplayOn)
    # pocket mode / raise-to-wake (com.fairphone6.Gestures)
    pocket = False                # fp6-gestures: proximity near and dark for > 1 s
    pocket_inhibit = False        # overridden by the user until the next "uncovered"
    pocket_blocked_at = None      # last refused power-key wake (monotonic)
    pocket_off_timer = None
    pocket_listeners = ()
    peek_id = 0
    iio_root = Path("/sys/bus/iio/devices")

    def __init__(self, bus, backlight, touch, curve, screen_power, sensors=True):
        self.bus, self.backlight, self.touch = bus, backlight, touch
        self.curve, self.screen_power, self.use_sensors = curve, screen_power, sensors
        self.lock = threading.RLock()
        self.user_value = max(1, backlight.read())   # last manual/auto target (logical)
        self.auto = False
        self.auto_offset = 0
        self.lux = LuxFilter()
        self.light_claimed = False
        self.prox_clients = set()
        self.prox_claimed = False
        self.prox_near = False
        self.screen_on = True
        self.off_reasons = set()      # "user", "proximity"
        # Display was switched on by a notification (Lomiri Notification.qml
        # sends "on" when one appears and "off" when it closes). Only then may
        # the closing "off" blank the display, as in repowerd; touch input or
        # any other on/off request clears it.
        self.notification_woke = False
        self.activity_watch = None
        self.dt2w = False
        self.ramp_target, self.ramp_id = None, 0
        self.listeners = []           # callables(status, reason)
        self.brightness_listeners = []
        self.proxy = None
        self.evdev_watch = None
        self.power_watch = None
        self.power_pressed_at = None
        if self.screen_power:
            self._start_power_key()
            self._start_volume_down()
            self._start_activity()
        if self.use_sensors:
            bus.add_signal_receiver(self._sensor_changed, signal_name="PropertiesChanged",
                                    dbus_interface=PROPERTIES, bus_name=SENSORS, path=SENSORS_PATH)
            bus.watch_name_owner(SENSORS, self._sensor_owner)

    # sensor proxy -----------------------------------------------------------
    def _sensor_iface(self):
        if self.proxy is None:
            self.proxy = dbus.Interface(self.bus.get_object(SENSORS, SENSORS_PATH), SENSORS)
        return self.proxy

    def _sensor_prop(self, name):
        obj = self.bus.get_object(SENSORS, SENSORS_PATH)
        return dbus.Interface(obj, PROPERTIES).Get(SENSORS, name, timeout=3)

    def _sensor_owner(self, owner):
        # iio-sensor-proxy restarted: claims are per-connection, re-claim.
        self.proxy = None
        if owner:
            with self.lock:
                if self.light_claimed:
                    self.light_claimed = False
                    self._update_light_claim()
                if self.prox_claimed:
                    self.prox_claimed = False
                    self._update_prox_claim()

    def has_light(self):
        if not self.use_sensors:
            return False
        try:
            return bool(self._sensor_prop("HasAmbientLight"))
        except dbus.DBusException:
            return False

    def _update_light_claim(self):
        want = self.auto and self.screen_on and self.use_sensors
        if want == self.light_claimed:
            return
        try:
            if want:
                self._sensor_iface().ClaimLight(timeout=5)
                self.light_claimed = True
                self._light(float(self._sensor_prop("LightLevel")))
            else:
                self._sensor_iface().ReleaseLight(timeout=5)
                self.light_claimed = False
        except dbus.DBusException as error:
            log("light claim change failed:", error.get_dbus_name())
            self.light_claimed = False

    def _update_prox_claim(self):
        want = bool(self.prox_clients) and self.use_sensors
        if want == self.prox_claimed:
            return
        try:
            if want:
                self._sensor_iface().ClaimProximity(timeout=5)
                self.prox_claimed = True
                self._proximity(bool(self._sensor_prop("ProximityNear")))
            else:
                self._sensor_iface().ReleaseProximity(timeout=5)
                self.prox_claimed = False
                self._proximity(False)
        except dbus.DBusException as error:
            log("proximity claim change failed:", error.get_dbus_name())
            self.prox_claimed = False

    def _sensor_changed(self, interface, changed, invalidated):
        if interface != SENSORS:
            return
        with self.lock:
            if "LightLevel" in changed and self.light_claimed:
                self._light(float(changed["LightLevel"]))
            if "ProximityNear" in changed and self.prox_claimed:
                self._proximity(bool(changed["ProximityNear"]))

    # auto brightness --------------------------------------------------------
    def _auto_target(self):
        base = curve_value(self.curve, self.lux.value or 0.0)
        return max(1, min(LOGICAL_MAX, base + self.auto_offset))

    def _light(self, lux):
        self.lux.push(lux)
        if self.auto and self.screen_on and not self.idle_dimmed:
            target = self._auto_target()
            current = self.ramp_target if self.ramp_target is not None else self.backlight.read()
            if needs_change(current, target):
                self._ramp_to(target)

    def set_auto(self, enabled):
        with self.lock:
            enabled = bool(enabled)
            if enabled == self.auto:
                return
            self.auto = enabled
            self.auto_offset = 0
            log("auto-brightness", "on" if enabled else "off")
            self._update_light_claim()
            if not enabled and self.screen_on:
                self._ramp_to(self.user_value)

    def _ramp_to(self, target, step_ms=30, step=6):
        """Smooth transition; superseded ramps stop at their next tick."""
        self.ramp_id += 1
        ramp, self.ramp_target = self.ramp_id, min(int(target), self.backlight.cap)

        def tick():
            with self.lock:
                if ramp != self.ramp_id or not self.screen_on:
                    return GLib.SOURCE_REMOVE
                current = self.backlight.read()
                if current == self.ramp_target:
                    self.ramp_target = None
                    return GLib.SOURCE_REMOVE
                delta = max(-step, min(step, self.ramp_target - current))
                value = max(1, min(LOGICAL_MAX, current + delta))
                try:
                    self._publish(self.backlight.write(value))
                except Exception as error:   # keep running; report once
                    log("backlight ramp write failed:", error)
                    self.ramp_target = None
                    return GLib.SOURCE_REMOVE
                if abs(self.ramp_target - value) <= 1:
                    self.ramp_target = None
                    return GLib.SOURCE_REMOVE
                return GLib.SOURCE_CONTINUE

        GLib.timeout_add(step_ms, tick)

    # manual brightness ------------------------------------------------------
    def set_user(self, value):
        with self.lock:
            value = int(value)
            if not 1 <= value <= LOGICAL_MAX:
                raise Invalid("Manual brightness must be between 1 and 255")
            if self.auto:
                # Slider while auto: keep it as an offset on the curve.
                self.auto_offset = max(-128, min(128, value - curve_value(self.curve, self.lux.value or 0.0)))
            self.user_value = value
            self.ramp_id += 1
            self.ramp_target = None
            if not self.screen_on:
                return value      # applied on screen-on
            try:
                confirmed = self.backlight.write(value)
            except dbus.DBusException:
                raise
            except Exception:
                raise Failed("Backlight write/readback failed") from None
            self._publish(confirmed)
            return confirmed

    def _publish(self, value):
        for listener in self.brightness_listeners:
            listener(value)

    def set_cap(self, cap):
        """Upper bound for every brightness write (manual, auto, ramps,
        screen-on). user_value is kept, so lifting the cap restores it."""
        with self.lock:
            cap = int(cap)
            if not 1 <= cap <= LOGICAL_MAX:
                raise Invalid("Brightness cap must be between 1 and 255")
            if cap == self.backlight.cap:
                return cap
            self.backlight.cap = cap
            log("brightness cap", cap)
            if self.screen_on:
                target = self._auto_target() if self.auto else self.user_value
                if min(target, cap) != self.backlight.read():
                    self._ramp_to(target)
            return cap

    # display power ----------------------------------------------------------
    def _apply_screen(self, reason):
        want_on = not self.off_reasons
        if want_on == self.screen_on:
            return
        self.screen_on = want_on
        if want_on:
            target = self._auto_target() if self.auto else self.user_value
            try:
                self.backlight.write(target)
            except Exception as error:
                log("screen-on brightness restore failed:", error)
            self._touch_lp(False)
        else:
            self.ramp_id += 1
            self.ramp_target = None
            try:
                self.backlight.blank()
            except Exception as error:
                log("screen-off blank failed:", error)
            # In LP the controller suppresses touches (cheek) and reports
            # double tap as KEY_WAKEUP.
            self._touch_lp(self.dt2w or "proximity" in self.off_reasons)
        self._update_light_claim()
        log("display", "on" if want_on else "off", "reason", reason, sorted(self.off_reasons))
        self._idle_screen_changed(want_on)
        for listener in self.listeners:
            listener(1 if want_on else 0, reason)

    def _touch_lp(self, enabled):
        if not self.touch.supported:
            return
        try:
            if enabled and not self.touch.gesture_enabled():
                # lp_mode requires a selected gesture; select double tap so
                # proximity blanking can still suppress touches.
                self.touch.set_double_tap(True)
            self.touch.set_lp(enabled)
            if not enabled and not self.dt2w and self.touch.gesture_enabled():
                self.touch.set_double_tap(False)
        except OSError as error:
            log("touch lp_mode change failed:", error)

    def _proximity(self, near):
        if near == self.prox_near:
            return
        self.prox_near = near
        if near and self.prox_clients:
            self.off_reasons.add("proximity")
        else:
            self.off_reasons.discard("proximity")
        self._apply_screen(REASON_PROXIMITY)

    def proximity_handling(self, name, enable):
        with self.lock:
            (self.prox_clients.add if enable else self.prox_clients.discard)(str(name))
            log("proximity handling", "enable" if enable else "disable", name, sorted(self.prox_clients))
            self._update_prox_claim()
            self._idle_rearm()

    def set_screen_power(self, mode, reason):
        if not self.screen_power:
            raise Unsupported("Display power is disabled in this backend (--screen-power)")
        with self.lock:
            if mode not in ("on", "off"):
                raise Invalid("mode must be 'on' or 'off'")
            if mode == "on" and self.pocket_refuses(int(reason)):
                return True
            if int(reason) in NOTIFICATION_REASONS:
                if mode == "on":
                    if self.screen_on:
                        return True
                    if "proximity" in self.off_reasons:
                        return True
                    self.notification_woke = True
                elif not self.notification_woke:
                    log("ignoring notification display off: display was not woken by it")
                    return True
                else:
                    self.notification_woke = False
            else:
                self.notification_woke = False
            if mode == "off":
                self.off_reasons.add("user")
            else:
                self.off_reasons.discard("user")
                # An explicit "on" (power key, double tap) also wins over
                # proximity, as in repowerd; notifications do not (above).
                self.off_reasons.discard("proximity")
            self._apply_screen(int(reason))
            return True

    # idle display-off (privacy-lock) ---------------------------------------
    # One "next check" timer: it fires at the dim or off point after the last
    # input and re-arms itself if input came in meanwhile (input only stamps
    # last_activity, so a stream of touch events costs no timer churn).
    def set_inactivity_timeouts(self, poweroff, dim):
        with self.lock:
            if int(poweroff) >= 0:
                self.idle_timeout = int(poweroff)
            if int(dim) >= 0:
                self.dim_timeout = int(dim)
            log("inactivity timeouts: display off", self.idle_timeout or "never",
                "dim", self.dim_timeout or "never")
            self._idle_cancel()
            self.idle_activity()

    def _idle_points(self):
        off = self.idle_timeout if self.screen_power else 0
        dim = self.dim_timeout if 0 < self.dim_timeout < off else 0
        return off, dim

    def _idle_blocked(self):
        return (not self.screen_on or bool(self.prox_clients)
                or bool(self.display_on_requests))

    def idle_activity(self, now=None):
        now = time.monotonic() if now is None else now
        with self.lock:
            self.last_activity = now
            if self.idle_dimmed:
                self._idle_undim()
            self._idle_arm(now)

    def _idle_cancel(self):
        if self.idle_timer is not None:
            GLib.source_remove(self.idle_timer)
            self.idle_timer = None

    def _idle_arm(self, now=None):
        if self.idle_timer is not None:
            return
        off, dim = self._idle_points()
        if not off or self._idle_blocked():
            return
        now = time.monotonic() if now is None else now
        due = dim if dim and not self.idle_dimmed else off
        self.idle_timer = self._schedule(max(0.2, self.last_activity + due - now), self._idle_fire)

    def _idle_rearm(self):
        with self.lock:
            self._idle_cancel()
            if self._idle_blocked():
                self._idle_undim()
            else:
                self._idle_arm()

    def _idle_fire(self, now=None):
        now = time.monotonic() if now is None else now
        with self.lock:
            self.idle_timer = None
            off, dim = self._idle_points()
            if not off or self._idle_blocked():
                self._idle_undim()
                return
            idle = now - self.last_activity
            if idle >= off - 0.05:
                log("inactivity: display off after", round(idle), "s")
                self.idle_dimmed = False
                self.notification_woke = False
                self.off_reasons.add("user")
                self._apply_screen(REASON_INACTIVITY)
                return
            if dim and not self.idle_dimmed and idle >= dim - 0.05:
                self._idle_dim()
            self._idle_arm(now)

    def _idle_dim(self):
        self.idle_dimmed = True
        self.ramp_id += 1
        self.ramp_target = None
        try:
            if self.backlight.read() > IDLE_DIM_VALUE:
                self.backlight.write(IDLE_DIM_VALUE)
        except Exception as error:
            log("idle dim failed:", error)

    def _idle_undim(self):
        if not self.idle_dimmed:
            return
        self.idle_dimmed = False
        if self.screen_on:
            try:
                self.backlight.write(self._auto_target() if self.auto else self.user_value)
            except Exception as error:
                log("idle undim failed:", error)

    def _idle_screen_changed(self, on):
        self._idle_cancel()
        self.idle_dimmed = False
        if on:
            self.idle_activity()

    def add_display_on_request(self, cookie, sender):
        with self.lock:
            if self.display_on_requests is None:
                self.display_on_requests = {}
            self.display_on_requests[int(cookie)] = str(sender or "")
            self._idle_rearm()

    def remove_display_on_requests(self, cookie=None, sender=None):
        with self.lock:
            requests = self.display_on_requests or {}
            for key in [k for k, v in requests.items()
                        if (cookie is not None and k == int(cookie)) or (sender is not None and v == sender)]:
                del requests[key]
            self._idle_rearm()
            self.idle_activity()

    # pocket mode / raise-to-wake (com.fairphone6.Gestures) --------------------
    # fp6-gestures decides "covered" (proximity near and dark for > 1 s) and
    # reports it; this side only gates. Every refused wake re-reads the
    # proximity sensor first, so an uncovered phone wakes at once even if the
    # "uncovered" report is still on its way. Calls (proximity handling) are
    # never touched: the gate is off and touch LP is left to the call logic.
    def _proximity_now(self):
        """Fresh near/far from the sensor's sysfs (True/False), None if unknown."""
        try:
            for entry in sorted(Path(self.iio_root).glob("iio:device*")):
                if (entry / "name").read_text().strip() != PROXIMITY_SENSOR:
                    continue
                raw = int((entry / "in_proximity_raw").read_text())
                level = int((entry / "in_proximity_nearlevel").read_text())
                return raw > level if level > 0 else None
        except (OSError, ValueError):
            pass
        return None

    def pocket_refuses(self, reason, now=None):
        if not self.pocket or self.screen_on or self.prox_clients:
            return False
        if reason == REASON_SNAP_DECISION:
            # Incoming calls and alarms still wake (touch stays in LP while
            # covered, so nothing is answered or dismissed in the pocket).
            return False
        if self._proximity_now() is False:
            log("pocket: sensor reads far at the wake: uncovered")
            self.pocket_blocked_at = None
            self.set_pocket(False)
            return False
        if reason == REASON_POWERKEY:
            now = time.monotonic() if now is None else now
            if self.pocket_blocked_at is not None and now - self.pocket_blocked_at <= POCKET_OVERRIDE_WINDOW:
                log("pocket: second power press: display on anyway")
                self.pocket_blocked_at = None
                self.set_pocket(False)
                self.pocket_inhibit = True
                return False
            self.pocket_blocked_at = now
        log("pocket: display stays off, reason", reason)
        return True

    def set_pocket(self, covered, now=None):
        now = time.monotonic() if now is None else now
        with self.lock:
            covered = bool(covered)
            if not covered:
                self.pocket_inhibit = False
            elif self.pocket_inhibit:
                return self.pocket
            if covered == self.pocket:
                return self.pocket
            self.pocket = covered
            log("pocket", "covered" if covered else "uncovered")
            self._pocket_cancel_off()
            self._pocket_touch()
            if covered:
                if self.screen_on and not self.prox_clients:
                    self.pocket_off_timer = self._schedule(POCKET_DISPLAY_OFF, self._pocket_display_off)
            else:
                blocked, self.pocket_blocked_at = self.pocket_blocked_at, None
                if blocked is not None and now - blocked <= POCKET_LATE_RELEASE and not self.screen_on:
                    log("pocket: uncovered right after a refused power press: display on")
                    self.set_screen_power("on", REASON_POWERKEY)
            for listener in self.pocket_listeners:
                listener(self.pocket)
            return self.pocket

    def _pocket_touch(self):
        """Touch LP while covered; otherwise what _apply_screen would choose."""
        if self.prox_clients:
            return
        if self.pocket:
            self._touch_lp(True)
        else:
            self._touch_lp(not self.screen_on and (self.dt2w or "proximity" in self.off_reasons))

    def _pocket_cancel_off(self):
        if self.pocket_off_timer is not None:
            GLib.source_remove(self.pocket_off_timer)
            self.pocket_off_timer = None

    def _pocket_display_off(self):
        with self.lock:
            self.pocket_off_timer = None
            if not self.pocket or not self.screen_on or self.prox_clients or self.display_on_requests:
                return
            log("pocket: display off")
            self.notification_woke = False
            self.off_reasons.add("user")
            self._apply_screen(REASON_INACTIVITY)

    def pocket_screen_changed(self, status, reason):
        # DisplayPowerStateChange listener: _apply_screen leaves LP on screen
        # on (an incoming call in the pocket); keep touches off while covered.
        if self.pocket:
            if not status:
                self._pocket_cancel_off()
            self._pocket_touch()

    def peek(self, seconds):
        """Raise to wake: display on (lock screen); off again after `seconds`
        without input. Counts as activity for the idle timer."""
        if not self.screen_power:
            raise Unsupported("Display power is disabled in this backend (--screen-power)")
        with self.lock:
            if self.screen_on or self.pocket or self.prox_clients or "proximity" in self.off_reasons:
                return False
            seconds = max(3, min(60, int(seconds)))
            self.notification_woke = False
            self.off_reasons.discard("user")
            self._apply_screen(REASON_UNKNOWN)
            if not self.screen_on:
                return False
            self.peek_id += 1
            peek, at = self.peek_id, self.last_activity
            self._schedule(seconds, lambda: self._peek_expired(peek, at))
            log("raise to wake: display on for", seconds, "s unless used")
            return True

    def _peek_expired(self, peek, at):
        with self.lock:
            if (peek != self.peek_id or not self.screen_on or self.last_activity > at + 0.01
                    or self.display_on_requests or self.prox_clients):
                return
            log("raise to wake: no input, display off again")
            self.off_reasons.add("user")
            self._apply_screen(REASON_INACTIVITY)

    # double tap -------------------------------------------------------------
    def set_dt2w(self, enabled):
        with self.lock:
            if not self.touch.supported:
                raise Unsupported("Kernel lacks ESWIN lp_mode (feature-push patch 0003)")
            self.dt2w = bool(enabled)
            try:
                self.touch.set_double_tap(self.dt2w)
            except OSError as error:
                raise Failed(f"gesture_wakeup write failed: {error}") from None
            log("double-tap-to-wake", "on" if self.dt2w else "off")
            if self.dt2w:
                self._start_evdev()

    def _start_evdev(self):
        if self.evdev_watch is not None:
            return
        node = self.touch.event_node()
        if node is None:
            log("ESWIN evdev node not found; KEY_WAKEUP not monitored")
            return
        fd = os.open(node, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)

        def readable(source, condition):
            try:
                data = os.read(fd, EVENT.size * 64)
            except BlockingIOError:
                return GLib.SOURCE_CONTINUE
            except OSError:
                os.close(fd)
                self.evdev_watch = None
                return GLib.SOURCE_REMOVE
            for off in range(0, len(data) - EVENT.size + 1, EVENT.size):
                _, _, etype, code, value = EVENT.unpack_from(data, off)
                if etype == EV_KEY and code == KEY_WAKEUP and value == 1:
                    self.wake_gesture()
            return GLib.SOURCE_CONTINUE

        self.evdev_watch = GLib.io_add_watch(fd, GLib.IO_IN | GLib.IO_ERR | GLib.IO_HUP, readable)
        log("monitoring", node, "for KEY_WAKEUP")

    # power key ----------------------------------------------------------------
    def _start_power_key(self, input_root=Path("/sys/class/input")):
        # Read-only and ungrabbed: Lomiri still receives the key for its dialog.
        node = None
        for entry in sorted(input_root.glob("event*")):
            try:
                if (entry / "device" / "name").read_text().strip() == POWER_KEY_NAME:
                    node = Path("/dev/input") / entry.name
                    break
            except OSError:
                continue
        if node is None:
            log("power key evdev node not found; short press does not toggle the display")
            return
        fd = os.open(node, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)

        def readable(source, condition):
            try:
                data = os.read(fd, EVENT.size * 64)
            except BlockingIOError:
                return GLib.SOURCE_CONTINUE
            except OSError:
                os.close(fd)
                self.power_watch = None
                return GLib.SOURCE_REMOVE
            for off in range(0, len(data) - EVENT.size + 1, EVENT.size):
                _, _, etype, code, value = EVENT.unpack_from(data, off)
                if etype == EV_KEY and code == KEY_POWER:
                    self.power_key(value)
            return GLib.SOURCE_CONTINUE

        self.power_watch = GLib.io_add_watch(fd, GLib.IO_IN | GLib.IO_ERR | GLib.IO_HUP, readable)
        log("monitoring", node, "for KEY_POWER")

    def _start_volume_down(self, input_root=Path("/sys/class/input")):
        # Read-only and ungrabbed, like the power key: only chord detection.
        for entry in sorted(input_root.glob("event*")):
            try:
                caps = (entry / "device" / "capabilities" / "key").read_text().split()
                bits = int("".join(f"{int(w, 16):016x}" for w in caps), 16)
            except (OSError, ValueError):
                continue
            if bits >> KEY_VOLUMEDOWN & 1:
                try:
                    fd = os.open(Path("/dev/input") / entry.name, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
                except OSError as error:
                    log("cannot open", entry.name, "for Volume-Down:", error)
                    continue
                break
        else:
            log("Volume-Down evdev node not found; Power+Volume-Down may toggle the display")
            return False

        def readable(source, condition):
            try:
                data = os.read(fd, EVENT.size * 64)
            except BlockingIOError:
                return GLib.SOURCE_CONTINUE
            except OSError:
                os.close(fd)
                return GLib.SOURCE_REMOVE
            for off in range(0, len(data) - EVENT.size + 1, EVENT.size):
                _, _, etype, code, value = EVENT.unpack_from(data, off)
                if etype == EV_KEY and code == KEY_VOLUMEDOWN:
                    self.volume_down(value)
            return GLib.SOURCE_CONTINUE

        GLib.io_add_watch(fd, GLib.IO_IN | GLib.IO_ERR | GLib.IO_HUP, readable)
        return True

    def volume_down(self, value):
        self.idle_activity()
        if value == 1:
            self.volume_down_held = True
            if self.power_pressed_at is not None:
                self.power_chord = True
        elif value == 0:
            self.volume_down_held = False

    def power_key(self, value, now=None):
        now = time.monotonic() if now is None else now
        self.idle_activity(now)
        if value == 1:
            self.power_pressed_at = now
            self.power_chord = self.volume_down_held
            self._multi_press_down()
            return
        if value != 0 or self.power_pressed_at is None:
            return
        held, self.power_pressed_at = now - self.power_pressed_at, None
        if self.power_chord:
            self.power_chord = False
            self._multi_reset()
            if self.screen_on:
                log("KEY_POWER with Volume-Down (screenshot chord): display unchanged")
                return
            # A chord from standby wakes at once; it never counts as a multi-press.
            if held < POWER_KEY_SHORT_PRESS:
                self._toggle_display()
            return
        if held >= POWER_KEY_SHORT_PRESS:
            self._multi_reset()
            return
        self._announce_press(now)
        self._short_press()

    def _announce_press(self, now):
        for listener in self.press_listeners:
            try:
                listener(now, self.screen_on)
            except Exception as error:
                log("power press listener failed:", error)

    def _toggle_display(self):
        with self.lock:
            off = self.screen_on and "user" not in self.off_reasons
            log("KEY_POWER short press:", "display off" if off else "display on")
        self.set_screen_power("off" if off else "on", REASON_POWERKEY)

    # power key double/triple press (gestures module, com.fairphone6.PowerKey).
    # multi_press_max 0 (default, nothing assigned): a short press toggles the
    # display at once. 2/3: presses whose release-to-press gap is under
    # POWER_KEY_MULTI_GAP count up; the count fires when it reaches the max or
    # the gap runs out. Count 1 toggles the display; 2 and 3 are announced
    # (MultiPress signal) instead and leave the display unchanged.
    def _short_press(self):
        if not self.multi_press_max:
            self._toggle_display()
            return
        self._multi_cancel_timer()
        self.multi_count += 1
        if self.multi_count >= self.multi_press_max:
            self._multi_fire()
        else:
            self.multi_timer = self._schedule(POWER_KEY_MULTI_GAP, self._multi_fire)

    def _schedule(self, seconds, callback):
        return GLib.timeout_add(int(seconds * 1000), lambda: (callback(), GLib.SOURCE_REMOVE)[1])

    def _multi_cancel_timer(self):
        if self.multi_timer is not None:
            GLib.source_remove(self.multi_timer)
            self.multi_timer = None

    def _multi_reset(self):
        self._multi_cancel_timer()
        self.multi_count = 0

    def _multi_press_down(self):
        # A new press inside the gap: wait for its release before deciding.
        self._multi_cancel_timer()

    def _multi_fire(self):
        count, self.multi_count, self.multi_timer = self.multi_count, 0, None
        if count == 1:
            self._toggle_display()
        elif count >= 2:
            log("KEY_POWER multi-press:", count)
            for listener in self.multi_listeners:
                listener(count, self.screen_on)

    def set_multi_press(self, maximum):
        maximum = int(maximum)
        if maximum not in (0, 2, 3):
            raise Invalid("maximum must be 0, 2 or 3")
        self.multi_press_max = maximum
        self._multi_reset()
        log("power key multi-press max:", maximum)

    def user_activity(self):
        with self.lock:
            if self.notification_woke and self.screen_on:
                log("touch input: display stays on after the notification")
            self.notification_woke = False

    def _start_activity(self):
        node = self.touch.event_node() if self.touch.supported else None
        if node is None:
            return
        try:
            fd = os.open(node, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
        except OSError as error:
            log("touch activity not monitored:", error)
            return

        def readable(source, condition):
            try:
                data = os.read(fd, EVENT.size * 64)
            except BlockingIOError:
                return GLib.SOURCE_CONTINUE
            except OSError:
                os.close(fd)
                self.activity_watch = None
                return GLib.SOURCE_REMOVE
            self.idle_activity()
            if self.notification_woke and any(
                    EVENT.unpack_from(data, off)[2] == EV_ABS
                    for off in range(0, len(data) - EVENT.size + 1, EVENT.size)):
                self.user_activity()
            return GLib.SOURCE_CONTINUE

        self.activity_watch = GLib.io_add_watch(fd, GLib.IO_IN | GLib.IO_ERR | GLib.IO_HUP, readable)

    def wake_gesture(self):
        with self.lock:
            self.notification_woke = False
            log("KEY_WAKEUP (double tap)")
            if self.pocket_refuses(REASON_UNKNOWN):
                return
            if "user" in self.off_reasons and "proximity" not in self.off_reasons:
                self.off_reasons.discard("user")
                self._apply_screen(REASON_UNKNOWN)


# --------------------------------------------------------------- D-Bus objects
class Repowerd(dbus.service.Object):
    def __init__(self, bus, core, authorizer):
        super().__init__(bus, REPOWERD_PATH)
        self.core, self.authorizer = core, authorizer
        self.last = core.backlight.read()
        self.cookies = 0
        core.brightness_listeners.append(self.publish)

    @dbus.service.method(REPOWERD, in_signature="", out_signature="(iiiib)")
    def getBrightnessParams(self):
        # (dim, min, max, default, autobrightness supported)
        return dbus.Struct((10, 1, LOGICAL_MAX, max(1, self.core.backlight.initial), self.core.has_light()),
                           signature="iiiib")

    def write(self, value, sender):
        self.authorizer.require(sender)
        self.core.set_user(value)
        # Re-announce even when unchanged so a settings slider that started
        # after a daemon restart (or missed a change) resyncs.
        self.publish(self.core.backlight.read(), force=True)

    @dbus.service.method(REPOWERD, in_signature="i", out_signature="", sender_keyword="sender")
    def setUserBrightness(self, value, sender=None):
        self.write(value, sender)

    @dbus.service.method(REPOWERD, in_signature="b", out_signature="", sender_keyword="sender")
    def userAutobrightnessEnable(self, enable, sender=None):
        self.authorizer.require(sender)
        self.core.set_auto(enable)

    @dbus.service.method(REPOWERD, in_signature="s", out_signature="", sender_keyword="sender")
    def enableProximityHandling(self, name, sender=None):
        self.authorizer.require(sender)
        self.core.proximity_handling(name, True)

    @dbus.service.method(REPOWERD, in_signature="s", out_signature="", sender_keyword="sender")
    def disableProximityHandling(self, name, sender=None):
        self.authorizer.require(sender)
        self.core.proximity_handling(name, False)

    # This backend never suspends: accept wakelock-style requests as no-ops so
    # callers (alarms, downloads) do not error out.
    @dbus.service.method(REPOWERD, in_signature="si", out_signature="s")
    def requestSysState(self, name, state):
        self.cookies += 1
        return f"fp6-{self.cookies}"

    @dbus.service.method(REPOWERD, in_signature="s", out_signature="")
    def clearSysState(self, cookie):
        pass

    @dbus.service.method(PROPERTIES, in_signature="ss", out_signature="v")
    def Get(self, interface, name):
        if interface != REPOWERD or name != "brightness":
            raise Invalid("Unknown property")
        return dbus.Int32(self.core.backlight.read(), variant_level=1)

    @dbus.service.method(PROPERTIES, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface != REPOWERD:
            raise Invalid("Unknown interface")
        return {"brightness": dbus.Int32(self.core.backlight.read())}

    @dbus.service.method(PROPERTIES, in_signature="ssv", out_signature="", sender_keyword="sender")
    def Set(self, interface, name, value, sender=None):
        if interface != REPOWERD or name != "brightness" or not isinstance(value, dbus.Int32):
            raise Invalid("Only the integer brightness property is writable")
        self.write(value, sender)

    @dbus.service.signal(PROPERTIES, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated):
        pass

    @dbus.service.method(INTROSPECTABLE, in_signature="", out_signature="s", path_keyword="object_path",
                         connection_keyword="connection")
    def Introspect(self, object_path=None, connection=None):
        node = ET.fromstring(super().Introspect(object_path, connection))
        interface = next(item for item in node.findall("interface") if item.get("name") == REPOWERD)
        ET.SubElement(interface, "property", name="brightness", type="i", access="readwrite")
        return ET.tostring(node, encoding="unicode")

    def publish(self, value, force=False):
        if force or value != self.last:
            self.last = value
            self.PropertiesChanged(REPOWERD, {"brightness": dbus.Int32(value)}, [])

    def poll(self):
        try:
            if self.core.screen_on and not self.core.idle_dimmed:
                self.publish(self.core.backlight.read())
        except Exception:
            os._exit(1)
        return GLib.SOURCE_CONTINUE


class Screen(dbus.service.Object):
    def __init__(self, bus, core, repowerd, authorizer):
        super().__init__(bus, SCREEN_PATH)
        self.core, self.repowerd, self.authorizer = core, repowerd, authorizer
        self.bus = bus
        self.requests = 0
        self.request_watches = {}
        core.listeners.append(lambda status, reason: self.DisplayPowerStateChange(status, reason))

    @dbus.service.method(SCREEN, in_signature="i", out_signature="", sender_keyword="sender")
    def setUserBrightness(self, value, sender=None):
        self.repowerd.write(value, sender)

    @dbus.service.method(SCREEN, in_signature="b", out_signature="", sender_keyword="sender")
    def userAutobrightnessEnable(self, enabled, sender=None):
        self.authorizer.require(sender)
        self.core.set_auto(enabled)

    @dbus.service.method(SCREEN, in_signature="si", out_signature="b", sender_keyword="sender")
    def setScreenPowerMode(self, mode, reason, sender=None):
        self.authorizer.require(sender)
        return self.core.set_screen_power(str(mode), reason)

    @dbus.service.method(SCREEN, in_signature="ii", out_signature="")
    def setInactivityTimeouts(self, poweroff_timeout, dimmer_timeout):
        # Lomiri calls this at startup and on every change (-1 = unchanged).
        self.core.set_inactivity_timeouts(poweroff_timeout, dimmer_timeout)

    @dbus.service.method(SCREEN, in_signature="", out_signature="b")
    def getDoubleTapToWakeSupported(self):
        return self.core.touch.supported

    @dbus.service.method(SCREEN, in_signature="b", out_signature="", sender_keyword="sender")
    def setDoubleTapToWakeEnabled(self, enabled, sender=None):
        self.authorizer.require(sender)
        if not enabled and not self.core.touch.supported:
            return   # Lomiri sends the default "false" at startup
        self.core.set_dt2w(enabled)

    @dbus.service.method(SCREEN, in_signature="", out_signature="i", sender_keyword="sender")
    def keepDisplayOn(self, sender=None):
        # Holds off the idle display-off until removed or the caller leaves.
        self.requests += 1
        self.core.add_display_on_request(self.requests, sender)
        if sender and sender not in self.request_watches:
            def owner(name, sender=sender):
                if not name and sender in self.request_watches:
                    self.request_watches.pop(sender).cancel()
                    self.core.remove_display_on_requests(sender=sender)
            self.request_watches[sender] = self.bus.watch_name_owner(sender, owner)
        return self.requests

    @dbus.service.method(SCREEN, in_signature="i", out_signature="")
    def removeDisplayOnRequest(self, cookie):
        self.core.remove_display_on_requests(cookie=cookie)

    @dbus.service.signal(SCREEN, signature="ii")
    def DisplayPowerStateChange(self, status, reason):
        pass


class PowerKey(dbus.service.Object):
    """Power key double/triple press for the gestures daemon (fp6-gestures)."""

    def __init__(self, bus, core, authorizer):
        super().__init__(bus, POWERKEY_PATH)
        self.bus, self.core, self.authorizer = bus, core, authorizer
        self.owner_watch = None
        core.multi_listeners = [lambda count, on: self.MultiPress(count, on)]
        core.press_listeners = [lambda now, on: self.Press(dbus.Double(now), on)]

    @dbus.service.method(POWERKEY_IFACE, in_signature="i", out_signature="", sender_keyword="sender")
    def SetMultiPress(self, maximum, sender=None):
        self.authorizer.require(sender)
        self.core.set_multi_press(maximum)
        if self.owner_watch is not None:
            self.owner_watch.cancel()
            self.owner_watch = None
        if int(maximum) and sender:
            # Back to the instant single press when the client goes away.
            def owner(name):
                if not name and self.owner_watch is not None:
                    self.owner_watch.cancel()
                    self.owner_watch = None
                    self.core.set_multi_press(0)
            self.owner_watch = self.bus.watch_name_owner(sender, owner)

    @dbus.service.method(POWERKEY_IFACE, in_signature="", out_signature="i")
    def GetMultiPress(self):
        return self.core.multi_press_max

    @dbus.service.signal(POWERKEY_IFACE, signature="ib")
    def MultiPress(self, count, display_on):
        pass

    # Every short press (CLOCK_MONOTONIC seconds, display state before it),
    # whatever the multi-press mode; the privacy-lock SOS counter listens.
    @dbus.service.signal(POWERKEY_IFACE, signature="db")
    def Press(self, monotonic, display_on):
        pass


class BrightnessCap(dbus.service.Object):
    """Battery-saver brightness cap (fp6-batteryd). 255 = no cap."""

    def __init__(self, bus, core, authorizer):
        super().__init__(bus, CAP_PATH)
        self.core, self.authorizer = core, authorizer

    @dbus.service.method(CAP_IFACE, in_signature="i", out_signature="", sender_keyword="sender")
    def SetCap(self, cap, sender=None):
        self.authorizer.require(sender)
        old = self.core.backlight.cap
        if self.core.set_cap(cap) != old:
            self.CapChanged(dbus.Int32(self.core.backlight.cap))

    @dbus.service.method(CAP_IFACE, in_signature="", out_signature="i")
    def GetCap(self):
        return self.core.backlight.cap

    @dbus.service.signal(CAP_IFACE, signature="i")
    def CapChanged(self, cap):
        pass


class Gestures(dbus.service.Object):
    """Pocket mode and raise-to-wake for fp6-gestures."""

    def __init__(self, bus, core, authorizer):
        super().__init__(bus, GESTURES_PATH)
        self.bus, self.core, self.authorizer = bus, core, authorizer
        self.owner_watch, self.owner = None, None
        core.pocket_listeners = [lambda covered: self.PocketChanged(covered)]
        core.listeners.append(core.pocket_screen_changed)

    @dbus.service.method(GESTURES_IFACE, in_signature="b", out_signature="b", sender_keyword="sender")
    def SetPocket(self, covered, sender=None):
        self.authorizer.require(sender)
        if covered and sender and sender != self.owner:
            # Never left "covered" by a client that went away.
            if self.owner_watch is not None:
                self.owner_watch.cancel()
            def owner(name, sender=sender):
                if not name and self.owner == sender:
                    self.owner_watch.cancel()
                    self.owner_watch, self.owner = None, None
                    self.core.set_pocket(False)
            self.owner = sender
            self.owner_watch = self.bus.watch_name_owner(sender, owner)
        return self.core.set_pocket(bool(covered))

    @dbus.service.method(GESTURES_IFACE, in_signature="", out_signature="b")
    def GetPocket(self):
        return self.core.pocket

    @dbus.service.method(GESTURES_IFACE, in_signature="i", out_signature="b", sender_keyword="sender")
    def Peek(self, seconds, sender=None):
        self.authorizer.require(sender)
        return self.core.peek(seconds)

    @dbus.service.signal(GESTURES_IFACE, signature="b")
    def PocketChanged(self, covered):
        pass


class Service:
    def __init__(self, bus, core, authorizer):
        self.names = [dbus.service.BusName(name, bus, do_not_queue=True) for name in (REPOWERD, SCREEN)]
        self.repowerd = Repowerd(bus, core, authorizer)
        self.screen = Screen(bus, core, self.repowerd, authorizer)
        self.powerkey = PowerKey(bus, core, authorizer)
        self.cap = BrightnessCap(bus, core, authorizer)
        self.gestures = Gestures(bus, core, authorizer)
        self.timer = GLib.timeout_add_seconds(2, self.repowerd.poll)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--uid", type=int, default=10000)
    parser.add_argument("--curve", type=parse_curve, default=DEFAULT_CURVE,
                        help="lux:value[,lux:value...] auto-brightness table (value 1..255)")
    parser.add_argument("--screen-power", action="store_true",
                        help="implement setScreenPowerMode by backlight 0 + touch LP")
    parser.add_argument("--double-tap-default", action="store_true",
                        help="arm double-tap-to-wake at startup (Lomiri can still turn it off)")
    # test hooks (not used by the NixOS unit)
    parser.add_argument("--session-bus", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--backlight-dir", type=Path, default=Path("/sys/class/backlight/ae94000.dsi.0"),
                        help=argparse.SUPPRESS)
    parser.add_argument("--touch-dir", type=Path, default=Path("/sys/bus/spi/devices/spi0.0"), help=argparse.SUPPRESS)
    parser.add_argument("--no-sensors", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--iio-root", type=Path, default=Core.iio_root, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.uid <= 0:
        parser.error("The phone user UID must be positive")
    DBusGMainLoop(set_as_default=True)
    if args.session_bus:
        backlight = Backlight(args.backlight_dir, compatible=None, expect_max=None)
        bus = dbus.SessionBus()
    else:
        backlight = Backlight(args.backlight_dir)
        bus = dbus.SystemBus()
    core = Core(bus, backlight, Touch(args.touch_dir), args.curve, args.screen_power, sensors=not args.no_sensors)
    core.iio_root = args.iio_root
    service = Service(bus, core, Authorizer(bus, args.uid, allow_any=args.session_bus))
    if args.double_tap_default and core.touch.supported:
        try:
            core.set_dt2w(True)
        except Exception as error:
            log("default double-tap-to-wake failed:", error)
    loop = GLib.MainLoop()
    for number in (signal.SIGTERM, signal.SIGINT):
        GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, number, lambda: (loop.quit(), GLib.SOURCE_REMOVE)[1])
    log("FP6 Lomiri display backend ready; dt2w kernel support:", core.touch.supported,
        "screen-power:", args.screen_power)
    loop.run()
    with core.lock:
        if core.light_claimed or core.prox_claimed:
            core.auto = False
            core.prox_clients.clear()
            core._update_light_claim()
            core._update_prox_claim()
        if not core.screen_on:
            core.off_reasons.clear()
            core._apply_screen(REASON_UNKNOWN)
    return service


if __name__ == "__main__":
    sys.exit(0 if main() else 1)
