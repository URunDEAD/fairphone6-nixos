#!/usr/bin/env python3
"""Laptop (x86) previews of Settings > Storage and > Backup, offscreen.

  nix-build test/qt-env.nix -o /tmp/fp6-qt && test/preview.py /tmp/fp6-qt OUTDIR

Everything real except the phone: the agent and the root helper (dry run:
nix commands are logged, generation links of the FIXTURE are unlinked) run
on a private session bus over the synthetic phone from make-fixture.py;
the pages are rendered with QT_QPA_PLATFORM=offscreen at the FP6's
1116x2412 (GRID_UNIT_PX=24) and grabbed with grabToImage. Also a smoke
test: every step checks the agent's answers.
"""
import json, os, shutil, subprocess, sys, tempfile, time

qt = os.path.abspath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
only = sys.argv[3:]
here = os.path.dirname(os.path.abspath(__file__))
os.makedirs(out, exist_ok=True)
scratch = os.environ.get("FP6_PREVIEW_TMP") or tempfile.mkdtemp(prefix="fp6-sb-preview-")
root = os.path.join(scratch, "phone")
qmlscene = f"{qt}/bin/fp6-qmlscene"
envsh = open(qmlscene).read()
agent_pkg = envsh.split("FP6_AGENT=")[1].split("\n")[0].strip()

subprocess.run([sys.executable, f"{here}/make-fixture.py", root], check=True, stdout=subprocess.DEVNULL)
home = f"{root}/home"
fakebin = f"{scratch}/bin"
os.makedirs(fakebin, exist_ok=True)
with open(f"{fakebin}/dconf", "w") as f:
    f.write("#!/bin/sh\nif [ \"$1\" = dump ]; then printf '[com/lomiri/touch/system]\\nsome-option=true\\n\\n[org/gnome/desktop/interface]\\ntext-scaling-factor=1.0\\n\\n[com/fairphone6/backup]\\nfolder=%s\\n' \"'x'\"; else cat >/dev/null; fi\n")
os.chmod(f"{fakebin}/dconf", 0o755)

env = dict(os.environ)
for k in ["QT_PLUGIN_PATH", "QML2_IMPORT_PATH", "QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE", "DBUS_SESSION_BUS_ADDRESS"]:
    env.pop(k, None)
bus = f"fp6sb-preview-{os.getpid()}"  # abstract socket: scratch paths exceed SUN_LEN
env.update({
    "HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "XDG_STATE_HOME": f"{home}/.local/state",
    "XDG_DATA_DIRS": f"{root}/share", "USER": "user",
    "FP6_STORAGE_SESSION_BUS": "1", "FP6_BACKUP_MEDIA_ROOTS": f"{root}/media", "FP6_BACKUP_NO_SYSTEMD": "1",
    "FP6_BACKUP_SCRYPT_LOG_N": "12", "FP6_STORAGE_FAKE_FS": "256000000000:229500000000", "FP6_STORAGE_AGENT_IDLE": "0",
    "PATH": f"{fakebin}:{env['PATH']}", "DBUS_SESSION_BUS_ADDRESS": f"unix:abstract={bus}", "TZ": "Europe/Bucharest",
    "LANG": "en_US.UTF-8", "LC_TIME": "ro_RO.UTF-8",
})
with open(f"{scratch}/bus.conf", "w") as f:
    f.write(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:abstract={bus}</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>""")
procs = []
procs.append(subprocess.Popen(["dbus-daemon", "--config-file", f"{scratch}/bus.conf", "--nofork"], env=env))
time.sleep(0.5)
s = f"{root}/sys"
helper = [f"{agent_pkg}/bin/fp6-storaged", "--session-bus", "--no-polkit", "--uid", str(os.getuid()), "--dry-run",
          "--profiles", f"{s}/profiles", "--booted", f"{s}/booted-system", "--running", f"{s}/current-system",
          "--boot-part", f"boot_a={s}/boot_a.img", "--boot-part", f"boot_b={s}/boot_b.img", "--nix-db", f"{s}/db.sqlite",
          "--gcroots", f"{s}/gcroots", "--journal-dir", f"{s}/journal", "--waydroid-images", f"{s}/waydroid/images",
          "--wifi-dir", f"{s}/nm", "--fixture-dead", f"{s}/dead.txt", "--idle-exit", "0"]
procs.append(subprocess.Popen(helper, env=env, stderr=open(f"{scratch}/helper.log", "w")))
procs.append(subprocess.Popen([f"{agent_pkg}/bin/fp6-storage-agent", "serve"], env=env, stderr=open(f"{scratch}/agent.log", "w")))
time.sleep(1.0)


def call(method, args=None):
    r = subprocess.run([f"{agent_pkg}/bin/fp6-storage-agent", "call", method, json.dumps(args or {})], env=env,
                       capture_output=True, text=True)
    if r.returncode:
        raise RuntimeError(f"{method}: {r.stderr.strip()}")
    return json.loads(r.stdout)


def wait_job():
    for _ in range(600):
        j = call("status")["job"]
        if j and j.get("state") != "running":
            return j
        time.sleep(0.3)
    raise RuntimeError("job timeout")


def check(cond, msg):
    if not cond:
        raise SystemExit("CHECK FAILED: " + msg)
    print("ok:", msg)


HARNESS = """import QtQuick 2.12
import QtQuick.Window 2.2
import Lomiri.Components 1.3
MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(100.5)
    applicationName: "lomiri-system-settings"
    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("%PAGE%")
    }
    function find(item, name) {
        if (!item) return null
        if (item.objectName === name) return item
        var ch = item.children || []
        for (var i = 0; i < ch.length; i++) { var r = find(ch[i], name); if (r) return r }
        if (item.contentItem && item.contentItem !== item) { var c = find(item.contentItem, name); if (c) return c }
        return null
    }
    function el(name) { var r = find(mv, name); if (!r) console.log("HARNESS: no " + name); return r }
    function top() { return pageStack.primaryPage }
    function u(name) { return "%DIR%/" + name }
    function push(url) { pageStack.addPageToNextColumn(top(), u(url), { sm: top().sm }) }
    function scroll(name, y) { var p = el(name); if (p && p.flickable) p.flickable.contentY = y }
    property var steps: [ %STEPS% ]
    property int step: 0
    Timer {
        interval: %DELAY%
        repeat: true
        running: true
        onTriggered: {
            if (mv.step < mv.steps.length) { var f = mv.steps[mv.step]; mv.step++; try { f() } catch (e) { console.log("HARNESS: step failed: " + e + " at " + e.fileName + ":" + e.lineNumber + " " + e.stack) } }
            else { running = false; mv.grabToImage(function(r) { r.saveToFile("%OUT%"); Qt.quit() }) }
        }
    }
}
"""


def shot(name, page, steps=(), delay=1500, theme="Ambiance"):
    if only and name not in only:
        return
    dirpage = {"storage": "storage", "backup": "backup"}[page]
    plugin = envsh.split("FP6_LSS_PLUGIN=")[1].split("\n")[0].strip()
    src = f"{plugin}/share/lomiri-system-settings/qml-plugins/{dirpage}/PageComponent.qml"
    # pages resolve siblings relative to PageComponent.qml
    qml = HARNESS.replace("%PAGE%", "file://" + src).replace("%DIR%", "file://" + os.path.dirname(src)).replace("%DELAY%", str(delay)) \
        .replace("%OUT%", f"{out}/{name}.png").replace("%STEPS%", ", ".join(f"function() {{ {s} }}" for s in steps))
    path = f"{scratch}/harness-{name}.qml"
    open(path, "w").write(qml)
    os.makedirs(f"{home}/.config/lomiri-ui-toolkit", exist_ok=True)
    open(f"{home}/.config/lomiri-ui-toolkit/theme.ini", "w").write(f"[General]\ntheme=Lomiri.Components.Themes.{theme}\n")
    e = dict(env, QT_QPA_PLATFORM="offscreen", GRID_UNIT_PX="24", QT_QUICK_BACKEND=os.environ.get("QT_QUICK_BACKEND", ""))
    if not e["QT_QUICK_BACKEND"]:
        e.pop("QT_QUICK_BACKEND")
    r = subprocess.run([qmlscene, path], env=e, capture_output=True, text=True, timeout=60)
    log = "\n".join(l for l in (r.stdout + r.stderr).splitlines() if "fontconfig" not in l.lower())
    bad = [l for l in log.splitlines() if ("Error" in l or "error" in l or "HARNESS" in l or "TypeError" in l or "ReferenceError" in l
                                            or "is not defined" in l or "Cannot" in l) and "Binding loop" not in l]
    open(f"{scratch}/{name}.log", "w").write(log)
    print(f"shot {name}: {'saved' if os.path.exists(out + '/' + name + '.png') else 'MISSING'}" + ("" if not bad else "\n  " + "\n  ".join(bad[:12])))


try:
    call("storage.scan")
    j = wait_job()
    st = call("status")
    check(j["state"] == "done", "scan finished")
    plan = st["storage"]["system"]["generations"]
    check(plan["certain"] and plan["deletable"] == [1, 2, 3], f"deletable generations {plan['deletable']} (4 = boot_b, 5 = current/booted)")
    check(st["storage"]["system"]["store"]["garbage"] == 1600 * 1000 * 1000, "garbage size from the fixture dead list")

    shot("01-storage", "storage", delay=1200)
    shot("02-storage-bottom", "storage", ["scroll('fp6StoragePage', 2000)"], delay=1000)
    shot("03-system", "storage", ["push('SystemPage.qml')"])
    shot("04-system-gc", "storage", ["push('SystemPage.qml')", "scroll('fp6StorageSystemPage', 1500)"])
    shot("05-system-confirm", "storage", ["push('SystemPage.qml')", "el('fp6DeleteGens').clicked()"])
    shot("06-apps", "storage", ["push('AppsPage.qml')"])
    shot("07-apps-confirm", "storage", ["push('AppsPage.qml')", "el('fp6ClearAll').clicked()"])

    call("storage.deleteGenerations", {"nums": [1, 2, 3, 4, 5]})
    j = wait_job()
    check(j["state"] == "done" and j["result"]["deleted"] == [1, 2, 3] and j["result"]["refused"] == [4, 5],
          f"delete-generations [1..5] deleted {j['result'].get('deleted')} refused {j['result'].get('refused')}")
    check(sorted(os.listdir(f"{root}/sys/profiles")) == ["system", "system-4-link", "system-5-link"], "boot_b and current links kept")
    shot("08-system-after-delete", "storage", ["push('SystemPage.qml')"])
    call("storage.gc")
    j = wait_job()
    check(j["state"] == "done", "gc (dry run) with GC roots for the kept toplevels")
    roots = sorted(os.readlink(f"{root}/sys/gcroots/{n}") for n in os.listdir(f"{root}/sys/gcroots"))
    check(any("dddd" in r for r in roots) and any("eeee" in r for r in roots), "boot_b and current pinned as GC roots")
    call("storage.clearCache", {"id": "pure-maps"})
    j = wait_job()
    check(j["state"] == "done" and not os.path.exists(f"{home}/.cache/pure-maps"), "pure-maps cache cleared")

    # ------------------------------------------------------------ backup
    shot("10-backup-new", "backup")
    shot("11-backup-passphrase", "backup", ["el('fp6SetPassphrase').clicked()"])
    try:
        call("backup.run")
        check(wait_job()["state"] == "failed", "backup refused without a passphrase")
    except RuntimeError:
        pass
    call("backup.setPassphrase", {"new": "preview passphrase"})
    call("backup.setConfig", {"folder": f"{root}/media/SD-CARD/Backups", "selection": {"wifi": True, "configDirs": ["morph-browser"]}})
    call("backup.run")
    j = wait_job()
    check(j["state"] == "done" and j["result"]["verified"], f"backup written and verified: {j.get('result', j).get('file', j.get('error'))}")
    archive = j["result"]["file"]
    check(os.path.exists(f"{root}/media/SD-CARD/Backups/fp6-backup-key.age"), "key file next to the backup")
    raw = open(archive, "rb").read()
    check(b"fake-preview-password" not in raw and b"Test Contact" not in raw, "no plaintext in the archive")
    shot("12-backup", "backup")
    shot("13-backup-bottom", "backup", ["scroll('fp6BackupPage', 3000)"])
    shot("14-restore-list", "backup", ["el('fp6RestoreOpen').clicked()"])
    pv = call("backup.preview", {"file": archive})
    check(set(pv["categories"]) == {"settings", "contacts", "messages", "wifi"}, f"preview categories {sorted(pv['categories'])}")
    shot("15-restore-preview", "backup",
         ["el('fp6RestoreOpen').clicked()", "0", "el('fp6Backup_" + os.path.basename(archive)[len("fp6-backup-"):-len(".tar.age")] + "').clicked()"],
         delay=1800)
    # restore messages into the live db after changing it
    import sqlite3
    c = sqlite3.connect(f"{home}/.local/share/history-service/history.sqlite")
    c.execute("DELETE FROM threads")
    c.commit()
    c.close()
    call("backup.restore", {"file": archive, "categories": ["messages", "wifi"]})
    j = wait_job()
    check(j["state"] == "done", f"restore done: {j.get('result', j.get('error'))}")
    c = sqlite3.connect(f"{home}/.local/share/history-service/history.sqlite")
    check(c.execute("SELECT count(*) FROM threads").fetchone()[0] == 23, "messages restored (23 threads)")
    check(any(n.startswith("messages-") for n in os.listdir(f"{home}/.local/share/fp6-backup/pre-restore")), "pre-restore copy kept")
    shot("16-backup-dark", "backup", theme="SuruDark")
    shot("09-storage-dark", "storage", theme="SuruDark")
finally:
    for p in reversed(procs):
        p.terminate()
    for p in procs:
        try:
            p.wait(5)
        except Exception:
            p.kill()
    print("logs and fixture:", scratch)
    if not os.environ.get("FP6_PREVIEW_KEEP"):
        shutil.rmtree(f"{root}", ignore_errors=True)
