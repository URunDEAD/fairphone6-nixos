#!/usr/bin/env python3
"""Mock org.ayatana.indicator.sound for the laptop previews.

Exports the same actions and the same phone menu layout as the patched
ayatana-indicator-sound (Silent Mode, Volume, Output section, Input
section, Sound Settings...) with FP6 device lists, on the session bus of
the environment. Activating output-device/input-device moves the tick,
as the real service does once the sound server reports the new default.

  mock-indicator.py SCENARIO     SCENARIO = music | call
"""
import json
import sys

from gi.repository import Gio, GLib

SCENARIOS = {
    # Music with a Bluetooth speaker and a USB-C headset connected.
    "music": {
        "outputs": [
            ("alsa_card.platform-sound|[Out] Speaker", "Phone speaker", "audio-speakers-symbolic"),
            ("alsa_card.usb-Apple_USB-C_to_3.5mm_Headphone_Jack_Adapter-00|analog-output-headphones",
             "USB-C to 3.5mm Headphone Jack Adapter", "audio-headphones-symbolic"),
            ("bluez_card.XX_XX_XX_XX_XX_XX|headset-output", "Test Speaker", "audio-headphones-symbolic"),
        ],
        "inputs": [
            ("alsa_card.platform-sound|[In] Mic1", "Built-in microphone", "audio-input-microphone-symbolic"),
            ("alsa_card.platform-sound|[In] Mic2", "Built-in microphone (top)", "audio-input-microphone-symbolic"),
            ("alsa_card.usb-Apple_USB-C_to_3.5mm_Headphone_Jack_Adapter-00|analog-input-mic",
             "USB-C to 3.5mm Headphone Jack Adapter", "audio-input-microphone-symbolic"),
            ("bluez_card.XX_XX_XX_XX_XX_XX|headset-input", "Test Speaker microphone", "audio-input-microphone-symbolic"),
        ],
        "active_output": 2,
        "active_input": 0,
        "apps": [
            {"key": "lomiri-music-app", "name": "Music", "icon": "multimedia-player", "playing": True, "recording": False,
             "output": "bluez_card.XX_XX_XX_XX_XX_XX|headset-output", "input": "default"},
            {"key": "firefox", "name": "Firefox", "icon": "firefox", "playing": True, "recording": False,
             "output": "default", "input": "default"},
            {"key": "lomiri-camera-app", "name": "Camera", "icon": "camera-app", "playing": False, "recording": True,
             "output": "default", "input": "alsa_card.platform-sound|[In] Mic2"},
            {"key": "morph-browser", "name": "Morph Browser", "icon": "morph-browser", "playing": False, "recording": False,
             "output": "alsa_card.platform-sound|[Out] Speaker", "input": "default"},
        ],
    },
    # During a call: the earpiece appears, the call is on the earpiece.
    "call": {
        "outputs": [
            ("alsa_card.platform-sound|[Out] Earpiece", "Earpiece", "phone-symbolic"),
            ("alsa_card.platform-sound|[Out] Speaker", "Phone speaker", "audio-speakers-symbolic"),
            ("bluez_card.XX_XX_XX_XX_XX_XX|headset-output", "Test Speaker", "audio-headphones-symbolic"),
        ],
        "inputs": [
            ("alsa_card.platform-sound|[In] Mic1", "Built-in microphone", "audio-input-microphone-symbolic"),
            ("alsa_card.platform-sound|[In] Mic2", "Built-in microphone (top)", "audio-input-microphone-symbolic"),
            ("bluez_card.XX_XX_XX_XX_XX_XX|headset-input", "Test Speaker microphone", "audio-input-microphone-symbolic"),
        ],
        "active_output": 0,
        "active_input": 0,
    },
}

scenario = SCENARIOS[sys.argv[1] if len(sys.argv) > 1 else "music"]
state = {"output": scenario["outputs"][scenario["active_output"]][0],
         "input": scenario["inputs"][scenario["active_input"]][0]}


def as_json(kind):
    devs = scenario["outputs" if kind == "output" else "inputs"]
    return json.dumps([{"id": i, "label": l, "icon": ic, "active": i == state[kind]} for i, l, ic in devs],
                      separators=(",", ":"))


group = Gio.SimpleActionGroup()


def add(name, ptype, value):
    a = Gio.SimpleAction.new_stateful(name, GLib.VariantType.new(ptype) if ptype else None, value)
    group.add_action(a)
    return a


add("root", None, GLib.Variant("a{sv}", {"title": GLib.Variant("s", "Sound"), "visible": GLib.Variant("b", True)}))
add("silent-mode", "b", GLib.Variant("b", False))
add("mute", None, GLib.Variant("b", False))
add("volume", "i", GLib.Variant("d", 0.55))
add("volume-sync", "t", GLib.Variant("t", 0))
add("mic-volume", None, GLib.Variant("d", 1.0))
add("high-volume", None, GLib.Variant("b", False))
add("indicator-shown", None, GLib.Variant("b", False))
group.add_action(Gio.SimpleAction.new("phone-settings", None))
lists = {k: add(k + "-devices", None, GLib.Variant("s", as_json(k))) for k in ("output", "input")}


def selector(kind):
    a = add(kind + "-device", "s", GLib.Variant("s", state[kind]))

    def choose(action, param):
        state[kind] = param.get_string()
        action.set_state(param)
        lists[kind].set_state(GLib.Variant("s", as_json(kind)))
    a.connect("activate", choose)
    a.connect("change-state", choose)


selector("output")
selector("input")

auto = add("switch-to-new-devices", "b", GLib.Variant("b", True))
auto.connect("activate", lambda a, p: a.set_state(p if p is not None else GLib.Variant("b", not a.get_state().get_boolean())))
auto.connect("change-state", lambda a, v: a.set_state(v))

apps = scenario.get("apps", [])


def apps_json():
    return json.dumps(apps, separators=(",", ":"))


apps_action = add("app-streams", None, GLib.Variant("s", apps_json()))
set_app = Gio.SimpleAction.new("set-app-device", GLib.VariantType.new("s"))


def on_set_app(_a, param):
    kind, key, dev = param.get_string().split("\t", 2)
    for a in apps:
        if a["key"] == key:
            a[kind] = dev
    apps_action.set_state(GLib.Variant("s", apps_json()))


set_app.connect("activate", on_set_app)
group.add_action(set_app)


def section(title, action, devs):
    s = Gio.Menu()
    h = Gio.MenuItem.new(title, None)
    h.set_attribute_value("x-ayatana-type", GLib.Variant("s", "org.ayatana.indicator.section"))
    s.append_item(h)
    for i, label, _icon in devs:
        it = Gio.MenuItem.new(label, None)
        it.set_action_and_target_value(action, GLib.Variant("s", i))
        s.append_item(it)
    return s


menu = Gio.Menu()
vol = Gio.Menu()
silent = Gio.MenuItem.new("Silent Mode", "indicator.silent-mode(true)")
silent.set_attribute_value("x-ayatana-type", GLib.Variant("s", "org.ayatana.indicator.switch"))
vol.append_item(silent)
slider = Gio.MenuItem.new("Volume (Bluetooth)" if sys.argv[1:2] != ["call"] else "Volume", "indicator.volume(0)")
slider.set_attribute_value("x-ayatana-type", GLib.Variant("s", "org.ayatana.indicator.slider"))
slider.set_attribute_value("min-value", GLib.Variant("d", 0.0))
slider.set_attribute_value("max-value", GLib.Variant("d", 1.0))
slider.set_attribute_value("step", GLib.Variant("d", 0.01))
slider.set_attribute_value("min-icon", Gio.ThemedIcon.new("audio-volume-low-zero").serialize())
slider.set_attribute_value("max-icon", Gio.ThemedIcon.new("audio-volume-high").serialize())
slider.set_attribute_value("x-ayatana-sync-action", GLib.Variant("s", "indicator.volume-sync"))
vol.append_item(slider)
menu.append_section(None, vol)
menu.append_section(None, section("Output", "indicator.output-device", scenario["outputs"]))
menu.append_section(None, section("Input", "indicator.input-device", scenario["inputs"]))
menu.append("Sound Settings…", "indicator.phone-settings")
root = Gio.Menu()
ri = Gio.MenuItem.new(None, "indicator.root")
ri.set_attribute_value("x-ayatana-type", GLib.Variant("s", "org.ayatana.indicator.root"))
ri.set_submenu(menu)
root.append_item(ri)


def on_bus(conn, _name):
    conn.export_action_group("/org/ayatana/indicator/sound", group)
    conn.export_menu_model("/org/ayatana/indicator/sound/phone", root)


Gio.bus_own_name(Gio.BusType.SESSION, "org.ayatana.indicator.sound", 0, on_bus, None, None)
GLib.MainLoop().run()
