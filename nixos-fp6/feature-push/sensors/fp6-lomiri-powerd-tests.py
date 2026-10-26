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
# no usable Volume-Down node (no device, or one that cannot be opened): no crash
with tempfile.TemporaryDirectory() as root:
    check(core._start_volume_down(Path(root)) is False, "no Volume-Down device: chord detection off, no crash")
    dev = Path(root) / "event9999" / "device" / "capabilities"
    dev.mkdir(parents=True)
    (dev / "key").write_text(f"{1 << (m.KEY_VOLUMEDOWN - 64):x} 0\n")
    check(core._start_volume_down(Path(root)) is False, "Volume-Down node that cannot be opened: no crash")

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

# idle display-off (privacy-lock): dim, then off with reason inactivity ----
class FakeBacklight:
    def __init__(self):
        self.value = 200
    def read(self):
        return self.value
    def write(self, value):
        self.value = value
        return value
core = object.__new__(m.Core)
core.lock, core.screen_power, core.screen_on = threading.RLock(), True, True
core.off_reasons, core.prox_clients, core.notification_woke = set(), set(), False
core.backlight, core.auto, core.user_value, core.ramp_id, core.ramp_target = FakeBacklight(), False, 200, 0, None
idle_timers = {}
def idle_schedule(seconds, callback):
    key = len(idle_timers) + 100
    idle_timers[key] = (seconds, callback)
    return key
core._schedule = idle_schedule
core._idle_cancel = lambda: (idle_timers.pop(core.idle_timer, None), setattr(core, "idle_timer", None))
offs = []
def idle_apply(reason):
    core.screen_on = not core.off_reasons
    offs.append(reason)
    core._idle_screen_changed(core.screen_on)
core._apply_screen = idle_apply
def fire(now):
    key = core.idle_timer
    check(key in idle_timers, f"idle timer armed at {now}")
    idle_timers.pop(key, None)
    core._idle_fire(now)
core.idle_activity(0.0)
check(core.idle_timer is None, "no idle timer while the timeout is 0 (never)")
core.set_inactivity_timeouts(60, -1); core.set_inactivity_timeouts(-1, 45)
core.idle_activity(1000.0)
check(idle_timers[core.idle_timer][0] == 45, f"first check at the dim point {idle_timers.get(core.idle_timer)}")
core.idle_activity(1020.0)
fire(1045.0)
check(not core.idle_dimmed and idle_timers[core.idle_timer][0] == 20, "input before the dim point re-arms for the rest")
fire(1065.0)
check(core.idle_dimmed and core.backlight.value == m.IDLE_DIM_VALUE, "dims at dim-timeout")
core.idle_activity(1070.0)
check(not core.idle_dimmed and core.backlight.value == 200, "touch while dimmed restores brightness")
fire(1115.0); fire(1130.0)
check(offs == [m.REASON_INACTIVITY] and not core.screen_on and "user" in core.off_reasons, "display off after activity-timeout (reason inactivity)")
check(core.idle_timer is None, "no idle timer while the display is off")
core.off_reasons.clear(); idle_apply(m.REASON_POWERKEY)
check(core.idle_timer is not None, "display on re-arms the idle timer")
core.add_display_on_request(7, ":1.5")
check(core.idle_timer is None, "keepDisplayOn holds off the idle display-off")
core.remove_display_on_requests(sender=":1.5")
check(core.idle_timer is not None and not core.display_on_requests, "requests of a caller that left are dropped")
core.prox_clients.add("lomiri-telephony-service-handler"); core._idle_rearm()
check(core.idle_timer is None, "no idle display-off during a call (proximity handling)")
core.prox_clients.clear(); core._idle_rearm()
check(core.idle_timer is not None, "idle timer back after the call")
core.set_inactivity_timeouts(0, -1)
check(core.idle_timer is None, "activity-timeout 0 = never")
# Press signal: every short press, also with multi-press assigned
presses = []
core.press_listeners = [lambda now, on: presses.append(now)]
core.set_screen_power = lambda mode, reason: None
core.power_pressed_at, core.power_chord, core.volume_down_held = None, False, False
core.multi_press_max, core.multi_count, core.multi_timer = 3, 0, None
core._multi_cancel_timer = lambda: None
for t in (200.0, 200.3, 200.6, 200.9, 201.2):
    core.power_key(1, now=t); core.power_key(0, now=t + 0.1)
check(len(presses) == 5, f"Press for each of 5 quick presses with multi-press 3 {presses}")
core.power_key(1, now=210.0); core.power_key(0, now=212.0)
check(len(presses) == 5, "long press is not a Press")


# pocket mode (gestures: com.fairphone6.Gestures) -----------------------------
core = object.__new__(m.Core)
core.lock, core.screen_power = threading.RLock(), True
core.screen_on, core.off_reasons, core.notification_woke = False, {"user"}, False
core.prox_clients, core.dt2w, core.display_on_requests = set(), False, None
core.power_pressed_at, core.listeners, core.last_activity = None, [], 0.0
applied, lp = [], []
def fake_apply(reason):
    core.screen_on = not core.off_reasons
    applied.append((core.screen_on, reason))
    for listener in core.listeners:
        listener(1 if core.screen_on else 0, reason)
core._apply_screen = fake_apply
core._touch_lp = lambda enabled: lp.append(enabled)
timers = {}
def fake_schedule(seconds, callback):
    key = len(timers) + 100
    timers[key] = (seconds, callback)
    return key
core._schedule = fake_schedule
real_source_remove = m.GLib.source_remove
m.GLib.source_remove = lambda key: timers.pop(key, None)
core.listeners.append(core.pocket_screen_changed)
pockets = []
core.pocket_listeners = [pockets.append]
iio = Path(tempfile.mkdtemp()) / "iio:device0"
iio.mkdir()
(iio / "name").write_text("stk36c61\n")
(iio / "in_proximity_nearlevel").write_text("25\n")
def prox(raw):
    (iio / "in_proximity_raw").write_text(f"{raw}\n")
prox(200)
core.iio_root = iio.parent
check(core._proximity_now() is True, "fresh proximity read: near above nearlevel")
prox(3)
check(core._proximity_now() is False, "fresh proximity read: far")
prox(200)
core.set_pocket(True, now=0.0)
check(core.pocket and pockets == [True] and lp[-1] is True, "covered: touch LP, PocketChanged")
core.set_screen_power("on", m.REASON_POWERKEY)
check(not core.screen_on and not applied, "covered: power key does not wake")
core.wake_gesture()
check(not core.screen_on and not applied, "covered: double tap does not wake")
core.set_screen_power("on", m.REASON_NOTIFICATION)
check(not core.screen_on, "covered: notification does not wake")
core.pocket_blocked_at = None
core.pocket_refuses(m.REASON_POWERKEY, now=10.0)
check(not core.pocket_refuses(m.REASON_POWERKEY, now=11.5) and not core.pocket and core.pocket_inhibit,
      "second power press within 3 s overrides")
core.set_pocket(True)
check(not core.pocket, "after an override, covered is ignored until uncovered")
core.set_pocket(False); core.set_pocket(True, now=20.0)
check(core.pocket and not core.pocket_inhibit, "uncovered then covered: pocket again")
core.pocket_refuses(m.REASON_POWERKEY, now=30.0)
check(core.pocket_refuses(m.REASON_POWERKEY, now=34.0) is True, "press 4 s later is refused again")
prox(0)
del applied[:]
core.set_screen_power("on", m.REASON_POWERKEY)
check(core.screen_on and not core.pocket and applied == [(True, m.REASON_POWERKEY)],
      "sensor already far at the press: uncovered and woken at once")
# late release: refused press, then "uncovered" within 0.6 s wakes
core.off_reasons.add("user"); fake_apply(m.REASON_POWERKEY); prox(200)
core.set_pocket(True, now=40.0)
core.pocket_refuses(m.REASON_POWERKEY, now=40.0)
del applied[:]
core.set_pocket(False, now=40.4)
check(core.screen_on and applied[-1] == (True, m.REASON_POWERKEY), "uncovered 0.4 s after a refused press: wakes")
check(lp[-1] is False, "uncovered with the display on: touch LP off")
# incoming call (snap decision) still wakes, touch stays LP while covered
core.off_reasons.add("user"); fake_apply(m.REASON_POWERKEY)
core.set_pocket(True, now=50.0); del lp[:]
core.set_screen_power("on", m.REASON_SNAP_DECISION)
check(core.screen_on and lp and lp[-1] is True, "covered: incoming call wakes, touches stay off")
check(not timers, "no pocket display-off timer for a call screen")
core.set_pocket(False)
check(lp[-1] is False, "uncovered during the call screen: touch back")
# a lit display that gets covered goes off after 5 s (unless a call)
core.set_pocket(True)
key = [k for k, v in timers.items() if v[0] == m.POCKET_DISPLAY_OFF]
check(len(key) == 1, "covered with the display on: off timer armed")
timers.pop(key[0])[1]()
check(not core.screen_on and applied[-1] == (False, m.REASON_INACTIVITY), "covered display goes off")
core.set_pocket(False)
core.set_pocket(True); core.off_reasons.discard("user")
core.prox_clients.add("call")
check(not core.pocket_refuses(m.REASON_POWERKEY), "in a call the pocket gate is off")
core.prox_clients.clear(); core.set_pocket(False)
# raise to wake (Peek)
core.screen_on, core.off_reasons = False, {"user"}; timers.clear()
core.set_pocket(True)
check(core.peek(10) is False and not core.screen_on, "Peek refused while covered")
core.set_pocket(False)
check(core.peek(10) is True and core.screen_on, "Peek turns the display on")
(sec, expire), = [v for v in timers.values()]
check(sec == 10, "Peek timeout armed")
expire()
check(not core.screen_on and applied[-1] == (False, m.REASON_INACTIVITY), "Peek without input: display off again")
core.peek(10); (sec, expire), = [v for k, v in list(timers.items())[-1:]]
core.last_activity += 1.0
expire()
check(core.screen_on, "Peek then touch input: display stays on")
check(core.peek(10) is False, "Peek with the display already on does nothing")
m.GLib.source_remove = real_source_remove

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

# brightness cap (battery saver): clamps manual/screen-on, keeps the slider value
cap = dbus.Interface(bus.get_object(m.REPOWERD, m.CAP_PATH), m.CAP_IFACE)
check(int(cap.GetCap()) == 255, "BrightnessCap defaults to 255 (no cap)")
capped = []
bus.add_signal_receiver(lambda c: capped.append(int(c)), signal_name="CapChanged", dbus_interface=m.CAP_IFACE)
cap.SetCap(128)
GLib.timeout_add(600, loop.quit)
loop.run()
check((bl / "brightness").read_text().strip() == str((128 * 4095 + 127) // 255), "cap lowers the current brightness")
check(capped == [128], f"CapChanged signal {capped}")
rep.setUserBrightness(200)
check((bl / "brightness").read_text().strip() == str((128 * 4095 + 127) // 255), "manual brightness above the cap is clamped")
del events[:]
scr.setScreenPowerMode("off", 2); scr.setScreenPowerMode("on", 2)
GLib.timeout_add(300, loop.quit)
loop.run()
check((bl / "brightness").read_text().strip() == str((128 * 4095 + 127) // 255), "screen-on restore respects the cap")
check(events == [(0, 2), (1, 2)], f"only the explicit off/on emit DisplayPowerStateChange {events}")
cap.SetCap(255)
GLib.timeout_add(1200, loop.quit)
loop.run()
check((bl / "brightness").read_text().strip() == str((200 * 4095 + 127) // 255), "lifting the cap restores the slider value")
for bad in (0, 256):
    try:
        cap.SetCap(bad)
        check(False, f"cap {bad} rejected")
    except dbus.DBusException:
        check(True, f"cap {bad} rejected")
rep.setUserBrightness(255)
check(events == [(0, 2), (1, 2)], f"SetCap and capped writes emit no DisplayPowerStateChange {events}")
scr.setInactivityTimeouts(60, 30)
check(True, "setInactivityTimeouts accepted")
cookie = int(scr.keepDisplayOn())
scr.removeDisplayOnRequest(cookie)
check(cookie > 0, "keepDisplayOn/removeDisplayOnRequest")
scr.setInactivityTimeouts(0, 0)

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

# com.fairphone6.Gestures over D-Bus ------------------------------------------
(bl / "brightness").write_text("2048\n")
daemon = subprocess.Popen([sys.executable, str(Path(m.__file__)), "--session-bus", "--no-sensors",
                           "--screen-power", "--backlight-dir", str(bl), "--touch-dir", str(touch),
                           "--iio-root", str(iio.parent)],
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
for _ in range(100):
    if bus.name_has_owner(m.REPOWERD):
        break
    time.sleep(0.05)
ges = dbus.Interface(bus.get_object(m.REPOWERD, m.GESTURES_PATH), m.GESTURES_IFACE)
scr = dbus.Interface(bus.get_object(m.SCREEN, m.SCREEN_PATH), m.SCREEN)
prox(200)
scr.setScreenPowerMode("off", 2)
check(bool(ges.SetPocket(True)) and bool(ges.GetPocket()), "SetPocket over D-Bus")
check((touch / "lp_mode").read_text().startswith("1"), "covered: touch LP over D-Bus")
scr.setScreenPowerMode("on", 2)
settle(200)
check((bl / "brightness").read_text().strip() == "0", "covered: setScreenPowerMode on refused (display stays off)")
check(not bool(ges.Peek(10)), "Peek refused while covered")
settle(700)
ges.SetPocket(False)
check(bool(ges.Peek(10)) and int((bl / "brightness").read_text()) > 0, "Peek wakes when uncovered")
scr.setScreenPowerMode("off", 2)
client = subprocess.run([sys.executable, "-c", "import dbus; b=dbus.SessionBus(); dbus.Interface(b.get_object(%r, %r), %r).SetPocket(True)" % (m.REPOWERD, m.GESTURES_PATH, m.GESTURES_IFACE)])
settle(300)
check(client.returncode == 0 and not bool(ges.GetPocket()), "pocket clears when its client leaves the bus")
daemon.terminate()
out, _ = daemon.communicate(timeout=10)
check(daemon.returncode == 0, "clean shutdown (gestures)")
print(out)
sys.exit(1 if failures else 0)
