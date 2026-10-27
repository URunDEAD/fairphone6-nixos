#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""fp6-callrec end-to-end test on the laptop (no phone, no modem, no audio
hardware). Run inside test/shell.nix:

    nix-shell --pure --keep HOME test/shell.nix --run 'python3 test/e2e.py BIN'

Pieces:
  * private D-Bus system + session buses (python-dbusmock), with the
    dbusmock oFono template as the modem (calls to +999 numbers only);
  * a private PipeWire daemon (no session manager) with a null "speaker"
    sink, a virtual "mic" source and the "default" metadata;
  * "imsd": pw-play --media-role Phone playing 1000 Hz into the speaker
    (the far end) and pw-record --media-role Phone reading the mic, which
    carries 440 Hz (the user). Links are made by hand, as WirePlumber
    would;
  * a contacts database with one test contact.

Checks:
  A. auto-all + announce: dial, connect, 4 s, hang up -> two FLAC files and
     a sidecar; uplink = 440 Hz, downlink = 1000 Hz; the 1400 Hz tone is in
     the uplink recording AND in what "imsd" sent; names, 0600/0700,
     sidecar fields.
  B. manual: the D-Bus action "recording" toggles on/off during a call.
  C. record-now --path fallback: mic -> uplink 440 Hz, speaker monitor ->
     downlink 1000 Hz.
  D. delete via the "delete" action; mode off -> nothing is recorded.
"""
import json
import math
import os
import shutil
import sqlite3
import stat
import struct
import subprocess
import sys
import tempfile
import time
import wave

import dbus
import dbusmock

BIN = os.path.abspath(sys.argv[1])
NUMBER = "+99955501"
LOG = []


def log(*a):
    s = " ".join(str(x) for x in a)
    LOG.append(s)
    print(s, flush=True)


def check(cond, what):
    log(("PASS " if cond else "FAIL ") + what)
    if not cond:
        raise SystemExit(f"FAILED: {what}")


def sine_wav(path, freq, rate, seconds, db=-10.0):
    amp = 32767 * 10 ** (db / 20)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        frames = bytearray()
        for i in range(int(rate * seconds)):
            frames += struct.pack("<h", int(amp * math.sin(2 * math.pi * freq * i / rate)))
        w.writeframes(bytes(frames))


def read_audio(path):
    """-> (rate, samples) for .flac/.wav/.opus (opus via opusdec)."""
    if path.endswith(".flac"):
        raw = subprocess.run(["flac", "-d", "-s", "-c", path], check=True, capture_output=True).stdout
        tmp = path + ".dec.wav"
        open(tmp, "wb").write(raw)
        path = tmp
    with wave.open(path, "rb") as w:
        rate = w.getframerate()
        data = w.readframes(w.getnframes())
    return rate, struct.unpack("<%dh" % (len(data) // 2), data)


def goertzel(samples, rate, freq):
    if not samples:
        return 0.0
    k = 2 * math.cos(2 * math.pi * freq / rate)
    s1 = s2 = 0.0
    for x in samples:
        s0 = x + k * s1 - s2
        s2, s1 = s1, s0
    p = s1 * s1 + s2 * s2 - k * s1 * s2
    return math.sqrt(max(p, 0)) / len(samples)


def dominant(samples, rate, cands=(440, 1000, 1400)):
    """The strongest candidate tone, or None when everything is near silence."""
    e = {f: goertzel(samples, rate, f) for f in cands}
    best = max(e, key=e.get)
    return (best if e[best] > 100 else None), e


def window(samples, rate, a, b):
    return samples[int(a * rate):int(b * rate)]


class Env:
    def __init__(self):
        self.tmp = tempfile.mkdtemp(prefix="fp6-callrec-e2e-")
        self.rt = os.path.join(self.tmp, "run")
        os.makedirs(self.rt, mode=0o700)
        self.home = os.path.join(self.tmp, "home")
        os.makedirs(self.home)
        self.rec = os.path.join(self.home, "Recordings", "Calls")
        self.procs = []
        os.environ["XDG_RUNTIME_DIR"] = self.rt
        os.environ["HOME"] = self.home
        os.environ["XDG_CONFIG_HOME"] = os.path.join(self.tmp, "config")
        os.environ["XDG_STATE_HOME"] = os.path.join(self.tmp, "state")
        os.environ.pop("PIPEWIRE_REMOTE", None)
        self.settings = os.path.join(self.tmp, "settings.json")
        os.environ["FP6_CALLREC_SETTINGS"] = self.settings
        os.environ["FP6_CALLREC_CONTACTS_DB"] = self.contacts_db()
        os.environ["FP6_CALLREC_CS_CONFIG"] = os.path.join(self.tmp, "no-cs.conf")

    def contacts_db(self):
        p = os.path.join(self.tmp, "contacts.db")
        c = sqlite3.connect(p)
        c.executescript("""
            CREATE TABLE Contacts (contactId INTEGER PRIMARY KEY, deleted DATETIME);
            CREATE TABLE PhoneNumbers (detailId INTEGER PRIMARY KEY, contactId INTEGER, phoneNumber TEXT, subTypes TEXT, normalizedNumber TEXT);
            CREATE TABLE DisplayLabels (detailId INTEGER PRIMARY KEY, contactId INTEGER UNIQUE, displayLabel TEXT, displayLabelGroup TEXT, displayLabelGroupSortOrder INTEGER);
            CREATE TABLE Names (detailId INTEGER PRIMARY KEY, contactId INTEGER UNIQUE, firstName TEXT, lastName TEXT);
            CREATE TABLE Nicknames (detailId INTEGER PRIMARY KEY, contactId INTEGER, nickname TEXT);
            INSERT INTO Contacts VALUES (1, NULL);
            INSERT INTO PhoneNumbers VALUES (1, 1, '+999 555 01', '', '+99955501');
            INSERT INTO DisplayLabels VALUES (1, 1, 'Test Contact', 'T', 0);
        """)
        c.commit()
        c.close()
        return p

    def write_settings(self, **kw):
        s = {"mode": "off", "format": "flac", "directory": self.rec}
        s.update({k.replace("_", "-"): v for k, v in kw.items()})
        json.dump(s, open(self.settings, "w"))

    def spawn(self, args, **kw):
        p = subprocess.Popen(args, **kw)
        self.procs.append(p)
        return p

    def pipewire(self):
        d = os.path.join(os.environ["XDG_CONFIG_HOME"], "pipewire", "pipewire.conf.d")
        os.makedirs(d)
        open(os.path.join(d, "50-e2e.conf"), "w").write("""
context.objects = [
  { factory = adapter args = { factory.name = support.null-audio-sink node.name = "test_speaker"
      node.description = "Test speaker" media.class = "Audio/Sink" audio.position = [ FL FR ] } }
  { factory = adapter args = { factory.name = support.null-audio-sink node.name = "test_mic"
      node.description = "Test microphone" media.class = "Audio/Source/Virtual" audio.position = [ MONO ] } }
]
""")
        # WirePlumber: port configuration, default nodes and stream linking,
        # as on the phone, but no device monitors (the laptop's sound card
        # and Bluetooth stay out of the test).
        w = os.path.join(os.environ["XDG_CONFIG_HOME"], "wireplumber", "wireplumber.conf.d")
        os.makedirs(w)
        open(os.path.join(w, "50-e2e.conf"), "w").write("""
wireplumber.profiles = {
  main = {
    monitor.alsa = disabled
    monitor.alsa-midi = disabled
    monitor.bluez = disabled
    monitor.bluez-midi = disabled
    monitor.v4l2 = disabled
    monitor.libcamera = disabled
  }
}
""")
        self.spawn(["pipewire"], stdout=subprocess.DEVNULL, stderr=open(os.path.join(self.tmp, "pipewire.log"), "w"))
        for _ in range(50):
            if os.path.exists(os.path.join(self.rt, "pipewire-0")):
                break
            time.sleep(0.1)
        time.sleep(0.5)
        self.spawn(["wireplumber"], stdout=subprocess.DEVNULL, stderr=open(os.path.join(self.tmp, "wireplumber.log"), "w"))
        time.sleep(2.0)

    def fake_imsd(self):
        far = os.path.join(self.tmp, "far_1000.wav")
        mic = os.path.join(self.tmp, "mic_440.wav")
        sine_wav(far, 1000, 16000, 120)
        sine_wav(mic, 440, 48000, 120)
        self.imsd_up = os.path.join(self.tmp, "imsd_uplink.raw")
        # the user's voice into the virtual mic
        self.spawn(["pw-play", "--target", "0", "-P", '{ node.name = "e2e-mic-feed" node.autoconnect = false }', mic])
        # imsd: far end (downlink) and mic reader (uplink), media.role Phone,
        # linked by WirePlumber to the default sink / source like the real ones
        self.spawn(["pw-play", "--media-role", "Phone", "-P", '{ node.name = "e2e-imsd-play" }', far])
        self.spawn(["pw-record", "--media-role", "Phone", "--raw", "--rate", "16000", "--channels", "1", "--format", "s16",
                    "-P", '{ node.name = "e2e-imsd-rec" }', self.imsd_up])
        time.sleep(1.5)
        outs = subprocess.run(["pw-link", "-o"], capture_output=True, text=True).stdout.split()
        feed = [o for o in outs if o.startswith("e2e-mic-feed:")]
        if not feed:
            raise SystemExit("no mic feed ports: " + " ".join(outs))
        r = subprocess.run(["pw-link", feed[0], "test_mic:input_MONO"], capture_output=True, text=True)
        if r.returncode:
            raise SystemExit("pw-link mic feed: " + r.stderr)
        time.sleep(0.5)
        links = subprocess.run(["pw-link", "-l"], capture_output=True, text=True).stdout
        log("links before the call:\n" + links)
        if "e2e-imsd-rec:input_MONO" not in links or "e2e-imsd-play:output_FL" not in links or "e2e-mic-feed" not in links:
            raise SystemExit("WirePlumber did not link the fake imsd streams")

    def stop(self):
        for p in reversed(self.procs):
            if p.poll() is None:
                p.terminate()
                try:
                    p.wait(5)
                except subprocess.TimeoutExpired:
                    p.kill()


def sidecars(rec):
    out = []
    for day in sorted(os.listdir(rec)) if os.path.isdir(rec) else []:
        for f in sorted(os.listdir(os.path.join(rec, day))):
            if f.endswith(".json"):
                out.append(os.path.join(rec, day, f))
    return out


def wait_for(fn, timeout=10, step=0.2):
    end = time.time() + timeout
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(step)
    return fn()


def main():
    env = Env()
    log("tmp", env.tmp)
    dbusmock.DBusTestCase.start_system_bus()
    dbusmock.DBusTestCase.start_session_bus()
    system = dbusmock.DBusTestCase.get_dbus(system_bus=True)
    session = dbusmock.DBusTestCase.get_dbus(system_bus=False)
    ofono_proc, _ = dbusmock.DBusTestCase.spawn_server_template("ofono", {}, stdout=subprocess.DEVNULL)
    env.procs.append(ofono_proc)
    modem = system.get_object("org.ofono", "/ril_0")
    vcm = dbus.Interface(modem, "org.ofono.VoiceCallManager")

    def set_state(path, state):
        obj = system.get_object("org.ofono", path)
        dbus.Interface(obj, dbusmock.MOCK_IFACE).UpdateProperties("org.ofono.VoiceCall", {"State": dbus.String(state)})
        dbus.Interface(obj, dbusmock.MOCK_IFACE).EmitSignal("org.ofono.VoiceCall", "PropertyChanged", "sv", ["State", dbus.String(state, variant_level=1)])

    env.pipewire()
    env.fake_imsd()
    env.write_settings(mode="auto-all", announce=True)
    daemon_log = open(os.path.join(env.tmp, "daemon.log"), "w")
    daemon = env.spawn([BIN, "run"], stderr=daemon_log, stdout=daemon_log)
    check(wait_for(lambda: session.name_has_owner("com.fairphone6.CallRecorder")), "daemon owns com.fairphone6.CallRecorder")
    actions = dbus.Interface(session.get_object("com.fairphone6.CallRecorder", "/com/fairphone6/CallRecorder"), "org.gtk.Actions")
    rec1 = dbus.Interface(session.get_object("com.fairphone6.CallRecorder", "/com/fairphone6/CallRecorder"), "com.fairphone6.CallRecorder1")

    def status():
        return json.loads(str(rec1.Status()))

    # ---- A: automatic, VoLTE tap, announcement
    log("== A: auto-all, announce, VoLTE (imsd PipeWire streams)")
    call = vcm.Dial(NUMBER, "")
    time.sleep(1.0)
    check(not status()["recording"], "not recording while dialing")
    set_state(call, "active")
    check(wait_for(lambda: status()["recording"], 8), "recording starts when the call connects")
    st = status()
    check(st["path"] == "volte", f"call path volte (got {st['path']})")
    check(st["auto"] is True, "started automatically")
    desc = actions.Describe("recording")
    check(bool(desc[0]) and bool(desc[2][0]), "action 'recording' enabled with state true")
    time.sleep(4.0)
    system.get_object("org.ofono", call).Hangup(dbus_interface="org.ofono.VoiceCall")
    check(wait_for(lambda: not status()["recording"], 8), "recording stops when the call ends")
    scs = wait_for(lambda: [s for s in sidecars(env.rec) if not json.load(open(s)).get("in_progress")], 5)
    check(len(scs) == 1, f"one finished recording ({len(scs)})")
    sc = json.load(open(scs[0]))
    day = os.path.dirname(scs[0])
    base = os.path.basename(scs[0])[:-5]
    log("sidecar", scs[0])
    check(base.endswith("_out_Test-Contact"), f"file stem has direction and contact ({base})")
    check(os.path.basename(day) == base[:10], "date folder")
    check(sc["call_path"] == "volte" and sc["direction"] == "outgoing", "sidecar path/direction")
    check(sc["remote"]["number"] == NUMBER and sc["remote"]["contact"] == "Test Contact", "sidecar remote party")
    check(sc["line"]["modem"] == "/ril_0" and sc["line"]["source"] == "ofono", "sidecar line identity")
    check(sc["codec"] == "flac" and sc["sample_rate"] == 16000, f"flac at the native 16 kHz ({sc['sample_rate']})")
    check(sc["duration_s"] and 3.5 < sc["duration_s"] < 9, f"duration {sc['duration_s']}")
    check(sc["announce"]["requested"] and sc["announce"]["uplink"], "announcement injected into imsd's uplink")
    check(any("e2e-imsd-play" in d for d in sc["devices"]["downlink"]), "downlink device = imsd playback stream")
    check(any("test_mic" in d for d in sc["devices"]["uplink"]), "uplink device = imsd's source (mic)")
    up = os.path.join(day, sc["tracks"]["uplink"]["file"])
    down = os.path.join(day, sc["tracks"]["downlink"]["file"])
    check(up.endswith("_uplink.flac") and down.endswith("_downlink.flac"), "track file names")
    for p in (up, down, scs[0]):
        check(stat.S_IMODE(os.stat(p).st_mode) == 0o600, f"0600 {os.path.basename(p)}")
    for d in (env.rec, day):
        check(stat.S_IMODE(os.stat(d).st_mode) == 0o700, f"0700 {d}")
    r, s_up = read_audio(up)
    r2, s_down = read_audio(down)
    lead = sc["tracks"]["uplink"]["lead_in_s"] or 0
    f_up, e_up = dominant(window(s_up, r, lead + 1.2, lead + 3.0), r)
    f_dn, e_dn = dominant(window(s_down, r2, lead + 1.2, lead + 3.0), r2)
    log("uplink energies", {k: round(v, 1) for k, v in e_up.items()})
    log("downlink energies", {k: round(v, 1) for k, v in e_dn.items()})
    check(f_up == 440, "uplink file carries the user's 440 Hz")
    check(f_dn == 1000, "downlink file carries the far end's 1000 Hz")
    check(e_dn[440] < e_dn[1000] / 20, "no mic in the downlink")
    check(e_up[1000] < e_up[440] / 20, "no far end in the uplink")
    beep_rec = goertzel(window(s_up, r, lead, lead + 1.0), r, 1400)
    check(beep_rec > 200, f"1400 Hz tone at the start of the uplink recording ({beep_rec:.0f})")
    raw = open(env.imsd_up, "rb").read()
    imsd = struct.unpack("<%dh" % (len(raw) // 2), raw[: len(raw) // 2 * 2])
    best = max(goertzel(window(imsd, 16000, t, t + 0.3), 16000, 1400) for t in [x * 0.1 for x in range(0, max(1, int(len(imsd) / 1600) - 3))])
    check(best > 200, f"1400 Hz tone reached imsd's capture stream (sent to the far end) ({best:.0f})")
    check(sc["tracks"]["uplink"]["dropouts"] == [] and sc["tracks"]["downlink"]["dropouts"] == [], "no dropouts")
    lst = json.loads(str(actions.Describe("recordings")[2][0]))
    check(len(lst) == 1 and lst[0]["id"] == sc["id"], "recordings action lists it")

    # ---- B: manual toggle over the org.gtk.Actions API (the dialer's button)
    log("== B: manual, Record button via org.gtk.Actions")
    env.write_settings(mode="manual")
    call = vcm.Dial(NUMBER, "")
    time.sleep(0.8)  # the recorder sees "dialing" first: direction out
    set_state(call, "active")
    time.sleep(1.5)
    check(not status()["recording"], "manual mode: nothing until the button")
    actions.Activate("recording", [], {})
    check(wait_for(lambda: status()["recording"], 8), "Activate('recording') starts")
    time.sleep(2.0)
    actions.Activate("recording", [], {})
    check(wait_for(lambda: not status()["recording"], 8), "Activate('recording') stops")
    system.get_object("org.ofono", call).Hangup(dbus_interface="org.ofono.VoiceCall")
    time.sleep(1.5)
    scs = sidecars(env.rec)
    man = [json.load(open(s)) for s in scs if json.load(open(s))["started_by"] == "user"]
    check(len(man) == 1 and man[0]["stop_reason"] == "stopped by the user", "manual recording saved")
    check(man[0]["direction"] == "outgoing" and "_out_" in man[0]["id"], "manual recording direction out")

    # ---- C: record-now, fallback path
    log("== C: record-now --path fallback")
    env.write_settings(mode="off", format="wav")
    out = subprocess.run([BIN, "record-now", "--seconds", "3", "--path", "fallback"], capture_output=True, text=True, timeout=30)
    log(out.stderr.strip())
    check(out.returncode == 0, "record-now exit 0")
    up, down, sc_path = out.stdout.split()
    sc = json.load(open(sc_path))
    check(sc["call_path"] == "fallback" and sc["started_by"] == "record-now", "fallback sidecar")
    check(sc["call_path_note"].startswith("FALLBACK"), "fallback clearly labelled")
    check("_dbg_record-now_" in up and up.endswith(".wav"), "record-now names, wav format")
    r, s_up = read_audio(up)
    r2, s_down = read_audio(down)
    f_up, _ = dominant(window(s_up, r, 1.0, 2.5), r)
    f_dn, _ = dominant(window(s_down, r2, 1.0, 2.5), r2)
    check(r == 48000, "fallback at 48 kHz")
    check(f_up == 440 and f_dn == 1000, f"fallback: mic -> uplink ({f_up}), speaker monitor -> downlink ({f_dn})")

    env.write_settings(mode="off", format="opus")
    out = subprocess.run([BIN, "record-now", "--seconds", "2", "--path", "fallback"], capture_output=True, text=True, timeout=30)
    check(out.returncode == 0, "record-now opus exit 0")
    up, down, sc_path = out.stdout.split()
    check(up.endswith(".opus") and open(up, "rb").read(4) == b"OggS" and open(down, "rb").read(4) == b"OggS", "opus files are Ogg")
    check(json.load(open(sc_path))["codec"] == "opus", "sidecar codec opus")

    # ---- D: delete, off
    log("== D: delete action, mode off")
    first = lst[0]["id"]
    actions.Activate("delete", [dbus.String(first)], {})
    time.sleep(1.5)
    check(not any(first.split("/")[1] in s for s in sidecars(env.rec)), "delete removed the recording")
    n = len(sidecars(env.rec))
    call = vcm.Dial("+99955502", "")
    set_state(call, "active")
    time.sleep(2.5)
    check(not status()["recording"] and len(sidecars(env.rec)) == n, "mode off: no recording")
    actions.Activate("recording", [], {})
    time.sleep(1.0)
    check("off" in status()["error"], f"button in mode off -> error ({status()['error']})")
    system.get_object("org.ofono", call).Hangup(dbus_interface="org.ofono.VoiceCall")

    daemon.terminate()
    daemon.wait(10)
    env.stop()
    daemon_log.close()
    log("daemon log:\n" + open(os.path.join(env.tmp, "daemon.log")).read())
    log("ALL PASSED")
    out = os.environ.get("E2E_LOG")
    if out:
        open(out, "w").write("\n".join(LOG) + "\n")
    shutil.rmtree(env.tmp, ignore_errors=True)


if __name__ == "__main__":
    try:
        main()
    finally:
        dbusmock.DBusTestCase.tearDownClass() if hasattr(dbusmock.DBusTestCase, "tearDownClass") else None
