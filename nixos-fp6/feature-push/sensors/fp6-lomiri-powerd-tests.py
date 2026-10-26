#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Hardware-free tests for fp6-lomiri-powerd (run inside dbus-run-session)."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

import fp6_lomiri_powerd as m

failures = 0


def check(cond, what):
    global failures
    print(("PASS " if cond else "FAIL ") + what, flush=True)
    if not cond:
        failures += 1


# pure policy -------------------------------------------------------------
c = m.DEFAULT_CURVE
check(m.curve_value(c, 0) == 12, "curve at 0 lux")
check(m.curve_value(c, 1e6) == 255, "curve clamps high")
vals = [m.curve_value(c, x) for x in (0, 1, 10, 100, 1000, 10000)]
check(vals == sorted(vals), "curve monotonic")
check(m.parse_curve("0:5,100:200") == ((0.0, 5), (100.0, 200)), "parse curve")
try:
    m.parse_curve("0:0,1:300")
    check(False, "reject bad curve")
except ValueError:
    check(True, "reject bad curve")
f = m.LuxFilter()
f.push(100)
up = f.push(1000)
down = m.LuxFilter(); down.push(1000); dn = down.push(100)
check(up - 100 > 1000 - dn, "filter rises faster than it falls")
check(not m.needs_change(100, 105) and m.needs_change(100, 120), "hysteresis")

# power key: short press toggles the display, long press is left to Lomiri
import threading
core = object.__new__(m.Core)
core.lock, core.screen_on, core.off_reasons, core.power_pressed_at = threading.RLock(), True, set(), None
calls = []
def fake_power(mode, reason):
    calls.append((mode, reason))
    core.screen_on = mode == "on"
    (core.off_reasons.discard if mode == "on" else core.off_reasons.add)("user")
core.set_screen_power = fake_power
core.power_key(1, now=10.0); core.power_key(0, now=10.2)
check(calls == [("off", m.REASON_POWERKEY)], f"short press turns display off {calls}")
core.power_key(1, now=20.0); core.power_key(0, now=20.1)
check(calls[-1] == ("on", m.REASON_POWERKEY), "second short press turns display on")
core.power_key(1, now=30.0); core.power_key(2, now=31.0); core.power_key(0, now=32.5)
check(len(calls) == 2, "long press is left to Lomiri's power dialog")
core.power_key(0, now=40.0)
check(len(calls) == 2, "release without press is ignored")
# Power + Volume-Down (screenshot chord) leaves the display alone, either order
core.volume_down(1); core.power_key(1, now=50.0); core.power_key(0, now=50.2); core.volume_down(0)
check(len(calls) == 2, "volume-down then power: no display toggle")
core.power_key(1, now=60.0); core.volume_down(1); core.volume_down(0); core.power_key(0, now=60.3)
check(len(calls) == 2, "power then volume-down: no display toggle")
core.power_key(1, now=70.0); core.power_key(0, now=70.2)
check(calls[-1] == ("off", m.REASON_POWERKEY), "plain short press after a chord still toggles")
n = len(calls)
core.volume_down(1); core.power_key(1, now=90.0); core.power_key(0, now=90.2); core.volume_down(0)
check(len(calls) == n + 1 and calls[-1] == ("on", m.REASON_POWERKEY), "chord with the display off still wakes it")

# power key double/triple press (gestures): deterministic fake timer
timers = {}
def fake_schedule(seconds, callback):
    key = len(timers) + 1
    timers[key] = callback
    return key
def run_timers():
    for key in list(timers):
        timers.pop(key)()
core._schedule = fake_schedule
core._multi_cancel_timer = lambda: (timers.pop(core.multi_timer, None), setattr(core, "multi_timer", None))
multi = []
core.multi_listeners = [lambda count, on: multi.append((count, on))]
core.screen_on, core.off_reasons = True, set()
n = len(calls)
core.power_key(1, now=100.0); core.power_key(0, now=100.1)
check(len(calls) == n + 1 and not timers, "default (max 0): single press toggles at once, no timer")
core.screen_on, core.off_reasons = True, set()
core.set_multi_press(3); n = len(calls)
core.power_key(1, now=110.0); core.power_key(0, now=110.1)
check(len(calls) == n and len(timers) == 1, "max 3: single press waits for the gap")
run_timers()
check(len(calls) == n + 1 and calls[-1] == ("off", m.REASON_POWERKEY) and not multi, "max 3: lone press toggles after the gap")
core.screen_on, core.off_reasons = True, set(); n = len(calls)
core.power_key(1, now=120.0); core.power_key(0, now=120.1); core.power_key(1, now=120.3); core.power_key(0, now=120.4)
run_timers()
check(multi == [(2, True)] and len(calls) == n, "double press emits MultiPress(2), display unchanged")
core.power_key(1, now=130.0); core.power_key(0, now=130.1); core.power_key(1, now=130.2); core.power_key(0, now=130.3)
core.power_key(1, now=130.4); core.power_key(0, now=130.5)
check(multi[-1] == (3, True) and not timers and len(calls) == n, "triple press at max fires at once")
core.set_multi_press(2)
core.power_key(1, now=140.0); core.power_key(0, now=140.1); core.power_key(1, now=140.2); core.power_key(0, now=140.3)
check(multi[-1] == (2, True) and not timers, "max 2: double press fires at once")
del multi[:]
core.volume_down(1); core.power_key(1, now=150.0); core.power_key(0, now=150.1); core.volume_down(0)
core.power_key(1, now=150.3); core.power_key(0, now=150.4)
check(not multi and len(timers) == 1, "chord is not counted as a press")
n = len(calls); run_timers()
check(not multi and len(calls) == n + 1, "press after a chord alone is a single press")
core.screen_on, core.off_reasons = True, set(); n = len(calls)
core.power_key(1, now=160.0); core.power_key(0, now=160.1); core.power_key(1, now=160.2); core.power_key(0, now=162.0)
check(not multi and not timers and core.multi_count == 0 and len(calls) == n, "long press is not counted and cancels the count")
core.set_multi_press(0)

# notifications: the closing "off" only blanks a display the notification woke
core = object.__new__(m.Core)
core.lock, core.screen_power = threading.RLock(), True
core.screen_on, core.off_reasons, core.notification_woke = True, set(), False
applied = []
def fake_apply(reason):
    core.screen_on = not core.off_reasons
    applied.append((core.screen_on, reason))
core._apply_screen = fake_apply
core.set_screen_power("on", m.REASON_NOTIFICATION); core.set_screen_power("off", m.REASON_NOTIFICATION)
check(core.screen_on and applied == [], "notification closing while in use leaves the display on")
core.set_screen_power("off", m.REASON_POWERKEY)
core.set_screen_power("on", m.REASON_NOTIFICATION)
check(core.screen_on and core.notification_woke, "notification wakes a dark display")
core.set_screen_power("off", m.REASON_SNAP_DECISION)
check(not core.screen_on, "closing notification blanks the display it woke")
core.set_screen_power("on", m.REASON_NOTIFICATION); core.user_activity()
core.set_screen_power("off", m.REASON_NOTIFICATION)
check(core.screen_on, "touch after a notification keeps the display on")
core.set_screen_power("off", m.REASON_POWERKEY); core.off_reasons.add("proximity")
core.set_screen_power("on", m.REASON_NOTIFICATION)
check(not core.screen_on, "notification does not wake a display blanked by proximity")

# D-Bus service ------------------------------------------------------------
tmp = Path(tempfile.mkdtemp())
bl = tmp / "bl"; bl.mkdir()
(bl / "max_brightness").write_text("4095\n")
(bl / "brightness").write_text("2048\n")
(bl / "type").write_text("raw\n")
touch = tmp / "touch"; touch.mkdir()
(touch / "gesture_wakeup").write_text("disable mode[0]\n")
(touch / "lp_mode").write_text("0\n")
(tmp / "input").mkdir()

daemon = subprocess.Popen([sys.executable, str(Path(m.__file__)), "--session-bus", "--no-sensors",
                           "--screen-power", "--backlight-dir", str(bl), "--touch-dir", str(touch)],
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
for _ in range(100):
    if bus.name_has_owner(m.SCREEN) and bus.name_has_owner(m.REPOWERD):
        break
    time.sleep(0.05)
check(bus.name_has_owner(m.SCREEN), "service owns both names")
rep = dbus.Interface(bus.get_object(m.REPOWERD, m.REPOWERD_PATH), m.REPOWERD)
scr = dbus.Interface(bus.get_object(m.SCREEN, m.SCREEN_PATH), m.SCREEN)

params = rep.getBrightnessParams()
check(tuple(int(x) for x in params[:3]) == (10, 1, 255) and not bool(params[4]),
      "getBrightnessParams without light sensor")
rep.setUserBrightness(255)
check((bl / "brightness").read_text().strip() == "4095", "manual brightness 255 -> raw 4095")
pk = dbus.Interface(bus.get_object(m.REPOWERD, m.POWERKEY_PATH), m.POWERKEY_IFACE)
pk.SetMultiPress(3)
check(int(pk.GetMultiPress()) == 3, "PowerKey.SetMultiPress over D-Bus")
try:
    pk.SetMultiPress(5)
    check(False, "PowerKey rejects max 5")
except dbus.DBusException:
    check(True, "PowerKey rejects max 5")
client = subprocess.run([sys.executable, "-c", "import dbus; b=dbus.SessionBus(); dbus.Interface(b.get_object(%r, %r), %r).SetMultiPress(2)" % (m.REPOWERD, m.POWERKEY_PATH, m.POWERKEY_IFACE)])
time.sleep(0.3)
check(client.returncode == 0 and int(pk.GetMultiPress()) == 0, "multi-press resets when its client leaves the bus")
check(bool(scr.getDoubleTapToWakeSupported()), "dt2w supported with lp_mode present")
scr.setDoubleTapToWakeEnabled(True)
check((touch / "gesture_wakeup").read_text().startswith("2"), "dt2w selects double tap")

events = []
bus.add_signal_receiver(lambda s, r: events.append((int(s), int(r))), signal_name="DisplayPowerStateChange",
                        dbus_interface=m.SCREEN)
check(bool(scr.setScreenPowerMode("off", 2)), "screen off accepted")
loop = GLib.MainLoop()
GLib.timeout_add(300, loop.quit)
loop.run()
check((bl / "brightness").read_text().strip() == "0", "screen off blanks backlight")
check((touch / "lp_mode").read_text().startswith("1"), "screen off enters touch LP with dt2w")
scr.setScreenPowerMode("on", 2)
GLib.timeout_add(300, loop.quit)
loop.run()
check((bl / "brightness").read_text().strip() == "4095", "screen on restores brightness")
check((touch / "lp_mode").read_text().startswith("0"), "screen on leaves touch LP")
check(events == [(0, 2), (1, 2)], f"DisplayPowerStateChange signals {events}")
scr.setInactivityTimeouts(60, 30)
check(True, "setInactivityTimeouts accepted")

daemon.terminate()
out, _ = daemon.communicate(timeout=10)
check(daemon.returncode == 0, "clean shutdown")
print(out)

# auto brightness + proximity against a scripted SensorProxy ---------------
fake = subprocess.Popen([sys.executable, str(Path(__file__).with_name("fake_sensorproxy.py"))])
for _ in range(100):
    if bus.name_has_owner(m.SENSORS):
        break
    time.sleep(0.05)
(bl / "brightness").write_text("2048\n")
daemon = subprocess.Popen([sys.executable, str(Path(m.__file__)), "--session-bus",
                           "--backlight-dir", str(bl), "--touch-dir", str(touch)],
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
for _ in range(100):
    if bus.name_has_owner(m.REPOWERD):
        break
    time.sleep(0.05)
sp = dbus.Interface(bus.get_object(m.SENSORS, m.SENSORS_PATH), m.SENSORS)
rep = dbus.Interface(bus.get_object(m.REPOWERD, m.REPOWERD_PATH), m.REPOWERD)
scr = dbus.Interface(bus.get_object(m.SCREEN, m.SCREEN_PATH), m.SCREEN)


def settle(ms=1500):
    GLib.timeout_add(ms, loop.quit)
    loop.run()


check(bool(rep.getBrightnessParams()[4]), "auto-brightness advertised with light sensor")
scr.userAutobrightnessEnable(True)
settle(200)
check(tuple(sp.Claims()) == (1, 0), "auto-brightness claims light")
settle()
raw0 = int((bl / "brightness").read_text())
check(abs(raw0 - (12 * 4095 + 127) // 255) <= 20, f"0 lux ramps to curve minimum (raw {raw0})")
sp.Inject("LightLevel", dbus.Double(20000.0))
settle()
sp.Inject("LightLevel", dbus.Double(20000.0))
settle()
raw1 = int((bl / "brightness").read_text())
check(raw1 > 3000, f"bright light raises brightness (raw {raw1})")
rep.userAutobrightnessEnable(False)
settle(200)
check(tuple(sp.Claims()) == (0, 0), "disabling auto releases light")

events.clear()
rep.enableProximityHandling("lomiri-telephony-service-handler")
settle(200)
check(tuple(sp.Claims()) == (0, 1), "proximity handling claims proximity")
sp.Inject("ProximityNear", True)
settle(300)
check((bl / "brightness").read_text().strip() == "0", "near blanks display during call")
check((touch / "lp_mode").read_text().startswith("1"), "near suppresses touch (LP)")
sp.Inject("ProximityNear", False)
settle(300)
check(int((bl / "brightness").read_text()) > 0, "far restores display")
check((touch / "lp_mode").read_text().startswith("0"), "far leaves touch LP")
rep.disableProximityHandling("lomiri-telephony-service-handler")
settle(200)
check(tuple(sp.Claims()) == (0, 0), "disable releases proximity")
check(events == [(0, 3), (1, 3)], f"proximity DisplayPowerStateChange {events}")

daemon.terminate()
out, _ = daemon.communicate(timeout=10)
fake.terminate()
fake.wait(timeout=10)
check(daemon.returncode == 0, "clean shutdown (sensors)")
print(out)
sys.exit(1 if failures else 0)
