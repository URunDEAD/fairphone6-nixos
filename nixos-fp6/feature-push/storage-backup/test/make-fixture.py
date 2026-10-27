#!/usr/bin/env python3
"""Synthetic phone for the laptop previews and the end-to-end test.

  make-fixture.py ROOT

ROOT/home         a home with photos, videos, music, documents, app caches
                  and data (allocated with fallocate, contents are zeros),
                  a history-service and a contacts sqlite database
ROOT/sys          system profile with 5 generations, booted/running links,
                  boot_a (a postmarketOS-style image) and boot_b (pins
                  generation 4) as Android boot image headers, a Nix store
                  database (sizes only), dead paths, journal, Waydroid
                  images, one NetworkManager Wi-Fi keyfile (fake password)
ROOT/media/SD     a "memory card" mount
No personal data: every name and value here is made up.
"""
import os, sqlite3, struct, sys

root = os.path.abspath(sys.argv[1])
home, sysd = f"{root}/home", f"{root}/sys"
MB = 1000 * 1000
GB = 1000 * MB


def alloc(path, size):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        if size:
            os.posix_fallocate(f.fileno(), 0, size)


def text(path, s):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(s)


# ---------------------------------------------------------------- home
for i in range(1, 9):
    alloc(f"{home}/Pictures/camera/IMG_20261020_{i:04d}.jpg", 410 * MB)
alloc(f"{home}/Pictures/Screenshots/screenshot-1.png", 2 * MB)
alloc(f"{home}/Videos/camera/VID_20261021_0001.mp4", 1900 * MB)
alloc(f"{home}/Videos/Screencasts/rec-1.mp4", 140 * MB)
alloc(f"{home}/Music/Album/track01.ogg", 9 * MB)
alloc(f"{home}/Music/Album/track02.ogg", 8 * MB)
text(f"{home}/Documents/notes.txt", "synthetic note\n")
alloc(f"{home}/Documents/manual.pdf", 6 * MB)
alloc(f"{home}/Downloads/nixos-minimal.iso", 1100 * MB)
for app, cache, data, conf in [
    ("morph-browser", 610 * MB, 180 * MB, 1 * MB),
    ("pure-maps", 820 * MB, 40 * MB, 0),
    ("camera.ubports", 95 * MB, 0, 0),
    ("lomiri-mediaplayer-app", 30 * MB, 2 * MB, 0),
    ("fragments", 4 * MB, 12 * MB, 0),
    ("UBports", 4 * MB, 0, 0),
]:
    alloc(f"{home}/.cache/{app}/cache.bin", cache)
    if data:
        alloc(f"{home}/.local/share/{app}/data.bin", data)
    if conf:
        alloc(f"{home}/.config/{app}/settings.bin", conf)
text(f"{home}/.config/morph-browser/morph-browser.conf", "[General]\nhomepage=about:blank\n")
text(f"{home}/.config/lomiri-system-settings/settings.ini", "[General]\nx=1\n")
alloc(f"{home}/.local/share/waydroid/data/userdata.img", 2400 * MB)

apps = f"{root}/share/applications"
for base, name, icon in [("morph-browser", "Morph Browser", "morph-browser"), ("lomiri-camera-app", "Camera", "camera-app"),
                         ("pure-maps", "Pure Maps", "maps-app"), ("lomiri-mediaplayer-app", "Media Player", "mediaplayer-app"),
                         ("fragments", "Fragments", "torrent")]:
    text(f"{apps}/{base}.desktop", f"[Desktop Entry]\nType=Application\nName={name}\nIcon={icon}\n")

hist = f"{home}/.local/share/history-service/history.sqlite"
os.makedirs(os.path.dirname(hist), exist_ok=True)
c = sqlite3.connect(hist)
c.execute("PRAGMA journal_mode=WAL")
c.execute("CREATE TABLE IF NOT EXISTS threads (accountId TEXT, threadId TEXT, type INT, count INT)")
c.executemany("INSERT INTO threads VALUES ('ofono/ofono/account0', ?, 0, 3)", [(f"thread-{i}",) for i in range(23)])
c.commit()
c.close()
cdb = f"{home}/.local/share/system/privileged/Contacts/qtcontacts-sqlite/contacts.db"
os.makedirs(os.path.dirname(cdb), exist_ok=True)
c = sqlite3.connect(cdb)
c.execute("CREATE TABLE IF NOT EXISTS Contacts (contactId INTEGER PRIMARY KEY, displayLabel TEXT)")
c.executemany("INSERT INTO Contacts (displayLabel) VALUES (?)", [(f"Test Contact {i}",) for i in range(57)])
c.commit()
c.close()

# ---------------------------------------------------------------- system
def sp(c, name):
    return f"/nix/store/{c * 32}-{name}"

H = [sp(c, f"nixos-system-fp6-26.05.{i}") for i, c in enumerate("abcde", 1)]
prof = f"{sysd}/profiles"
os.makedirs(prof, exist_ok=True)
for i, h in enumerate(H, 1):
    l = f"{prof}/system-{i}-link"
    if not os.path.islink(l):
        os.symlink(h, l)
    os.utime(l, (1792000000 + i * 86400 * 3,) * 2, follow_symlinks=False)
if not os.path.islink(f"{prof}/system"):
    os.symlink("system-5-link", f"{prof}/system")
for name, target in [("booted-system", H[4]), ("current-system", H[4])]:
    if not os.path.islink(f"{sysd}/{name}"):
        os.symlink(target, f"{sysd}/{name}")


def bootimg(cmd):
    b = bytearray(4096)
    b[0:8] = b"ANDROID!"
    struct.pack_into("<I", b, 36, 4096)
    struct.pack_into("<I", b, 40, 2)
    c = cmd.encode()
    b[64:64 + min(512, len(c))] = c[:512]
    if len(c) > 512:
        b[608:608 + len(c) - 512] = c[512:]
    return bytes(b)


with open(f"{sysd}/boot_a.img", "wb") as f:
    f.write(bootimg("console=ttyMSM0 pmos_boot_uuid=0000 pmos_root_uuid=0000 quiet"))
with open(f"{sysd}/boot_b.img", "wb") as f:
    f.write(bootimg("console=ttyMSM0,115200 quiet loglevel=3 mobile-nixos.generation=" + H[3]))

shared = [(sp("f", "glibc-2.44"), 40 * MB), (sp("g", "qtbase-5.15"), 120 * MB), (sp("h", "lomiri-0.5"), 300 * MB),
          (sp("i", "linux-fp6-7.2.0"), 450 * MB), (sp("j", "waydroid-images-support"), 90 * MB),
          (sp("k", "firefox-140"), 520 * MB)]
own = {h: [(sp("lmnopqr"[i], f"system-path-{i}"), [900, 1200, 600, 1400, 2700][i] * MB)] for i, h in enumerate(H)}
dead = [(sp("s", "lomiri-0.4-old"), 310 * MB), (sp("t", "linux-fp6-7.2.0-test"), 470 * MB),
        (sp("u", "rust-build-deps"), 820 * MB)]
other = [(sp("v", "user-profile-env"), 380 * MB)]
db = f"{sysd}/db.sqlite"
if os.path.exists(db):
    os.unlink(db)
c = sqlite3.connect(db)
c.executescript("CREATE TABLE ValidPaths (id integer primary key autoincrement not null, path text unique not null, hash text not null, registrationTime integer not null, deriver text, narSize integer, ultimate integer, sigs text, ca text);"
                "CREATE TABLE Refs (referrer integer not null, reference integer not null, primary key (referrer, reference));")
allp = [(h, 1 * MB) for h in H] + shared + [x for v in own.values() for x in v] + dead + other
for p, s in allp:
    c.execute("INSERT INTO ValidPaths (path, hash, registrationTime, narSize) VALUES (?, 'sha256:0', 0, ?)", (p, s))
def ref(a, b):
    c.execute("INSERT INTO Refs SELECT x.id, y.id FROM ValidPaths x, ValidPaths y WHERE x.path=? AND y.path=?", (a, b))
for h in H:
    for s, _ in shared[:4]:
        ref(h, s)
    for s, _ in own[h]:
        ref(h, s)
ref(H[4], shared[4][0])
ref(H[4], shared[5][0])
c.commit()
c.close()
text(f"{sysd}/dead.txt", "\n".join(p for p, _ in dead) + "\n")
alloc(f"{sysd}/journal/machine/system.journal", 390 * MB)
alloc(f"{sysd}/waydroid/images/system.img", 1500 * MB)
alloc(f"{sysd}/waydroid/images/vendor.img", 180 * MB)
text(f"{sysd}/nm/Example Home.nmconnection",
     "[connection]\nid=Example Home\nuuid=6a3b1f1e-0000-4000-8000-0000000000aa\ntype=wifi\n\n[wifi]\nssid=ExampleHome\n\n"
     "[wifi-security]\nkey-mgmt=wpa-psk\npsk=fake-preview-password\n")
text(f"{sysd}/nm/usb0.nmconnection", "[connection]\nid=usb0\nuuid=1\ntype=ethernet\n")
os.makedirs(f"{root}/media/SD-CARD", exist_ok=True)
print(root)
