# Share via link / QR

Temporary local-network file sharing for Fairphone 6, Mobile NixOS and Lomiri.
An iPhone, Android phone or computer connects using its browser. No account,
cloud service, telemetry, external web assets, or AirDrop support is involved.

**This version uses HTTP, not HTTPS.** Use trusted Wi-Fi or the phone's private
hotspot. HTTP exposes the invitation and transfers to anyone able to observe
that network. There is no certificate or fingerprint in HTTP mode.

## Build and run

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell --run 'cargo test --locked'
nix-build
```

Rust stable, Linux, Qt 5.15, Lomiri UI Toolkit 1.3 and Lomiri.Content 1.3 are
required. `nix-shell` supplies the toolchain and QML imports on this laptop.
`Cargo.lock` pins crate versions. `package.nix` uses `rustPlatform.buildRustPackage`
and works natively on x86_64-linux or aarch64-linux. A cross-evaluation check is
provided; a native aarch64 build and real-phone checks remain necessary.

Import the module into the phone's existing Lomiri NixOS configuration:

```nix
{
  imports = [ ./webshare/module.nix ];
  hardware.fairphone6.webshare = {
    enable = true;
    # Use the actual wireless/hotspot names reported on this device.
    openFirewallInterfaces = [ "wlan0" "ap0" ];
  };
}
```

The module installs the touch desktop entry, content-hub share target and a
systemd **user** service. It does not configure Lomiri itself, create a hotspot,
open a global firewall port, or expose a server before the user starts a share.
The GUI starts the user service when launched. The CLI expects it already running:

```sh
systemctl --user start fp6-webshare
fp6-webshare interfaces
fp6-webshare share --interface wlan0 /absolute/path/to/file.pdf
fp6-webshare share --interface ap0 --uploads --text --minutes 5
fp6-webshare status
fp6-webshare approve UPLOAD_ID
fp6-webshare reject UPLOAD_ID
fp6-webshare take-text
fp6-webshare stop
fp6-webshare gui /absolute/path/to/file.pdf
```

`share` prints a short URL and a terminal QR. In the app, choose files using
content-hub or select **Share via link** from another app's Share menu. Select
Wi-Fi/hotspot, review the expiry, and tap Start. Receiving uploads and text are
separate switches, both off initially. The browser's **Connect to phone** action
consumes the invitation; opening it alone does not. One browser can download all
selected files. Refresh/retry in that browser works while the share remains active.
A new browser requires a new share. Cookies must be enabled. No range/resume.

Uploads are staged privately, never exposed to the browser, and only saved after
**Keep file** on the phone. Each saved name has a random prefix, so existing files
are never overwritten. Approved files live in
`$XDG_DATA_HOME/fp6-webshare/received` (normally
`~/.local/share/fp6-webshare/received`), with mode 0600 in a mode-0700 directory.
Rejected, expired and stopped shares discard staged uploads. Received text stays
in memory until the phone user taps **Copy received text**. The CLI's `take-text`
consumes and prints it; the CLI does not interact with the clipboard.

A download-only session stops after all shared files finish streaming. A
receive-only session stops after the first approved upload or consumed text;
other unapproved items are discarded. Mixed send/receive sessions remain active
until Stop, browser Finish, network loss, or expiry (1–30 minutes, default 10).
The GUI can close while the daemon finishes the current session. App reopening
shows that session. Closing the app is not the same as Stop.

## API and limits

Session bus name/interface `com.fairphone6.WebShare1`, object
`/com/fairphone6/WebShare1`; introspection XML is in `data/`.

| Method | Arguments | Result |
| --- | --- | --- |
| Start | JSON string: `files`, `interface`, `minutes`, `uploads`, `text` | status JSON string |
| Stop | none | none |
| Status | none | status JSON string |
| Interfaces | none | interface list JSON string |
| Approve | upload ID string | saved path string |
| Reject | upload ID string | none |
| TakeText | none | received UTF-8 text |

Status includes the URL/QR, countdown, connection state, shared-file completion,
pending upload IDs/names/sizes, and pending text. It is available only locally.
The session bus account is trusted; it can perform the same actions as the UI.
Errors are D-Bus `org.freedesktop.DBus.Error.Failed` with a useful message.
The QML adapter's authenticated loopback endpoint is private implementation detail,
not a browser-facing API. Its secret is stored under `$XDG_RUNTIME_DIR/fp6-webshare`.

Limits: 32 selected regular files; 4 concurrent transfers; 32 HTTP connections;
10-second header deadline; 120 requests/minute per session; 100 MiB per upload;
60-second upload deadline; 8 pending uploads; one pending UTF-8 text up to 4 KiB.
Sharing binds both the device (SO_BINDTODEVICE) and a single private IPv4 address
on a kernel-recognized Wi-Fi/hotspot
interface; same-subnet clients only. Ethernet, cellular, VPN, loopback and public
addresses are rejected. Wi-Fi client isolation can prevent sharing. Do not expose
TCP 8765 with router port forwarding. See `DESIGN.md` for the threat model.

## Local verification

```sh
nix-shell --run 'cargo fmt --check && cargo test --locked && cargo clippy --all-targets -- -D warnings'
nix-shell --run 'bash scripts/dbus-smoke.sh'
nix-shell --run 'bash scripts/preview.sh'
nix-instantiate --eval --strict nix/module-check.nix
nix-build
```

Preview generation uses `QT_QPA_PLATFORM=offscreen` and Mesa software OpenGL under Xvfb;
PNGs are saved under `previews/`. UITK shapes require the OpenGL scenegraph;
QtQuick's `software` backend crashes with this toolkit, so the preview script
uses `LIBGL_ALWAYS_SOFTWARE=1` and leaves `QT_QUICK_BACKEND` unset. Tests use synthetic files and injected network
metadata, or loopback for transport/D-Bus tests. They do not contact a phone.

## Manual checklist on the real phone

- Build/install natively on aarch64 NixOS; start the user service and confirm the
  desktop entry is touch-enabled. Check `journalctl --user -u fp6-webshare`.
- Open the app under Lomiri. Confirm Suru controls, scrolling, rotation and Maliit
  numeric input. Confirm no upload/text opt-in is enabled initially.
- Share one file and multiple files from the file manager/gallery via content-hub;
  check names, canceled selections and cleanup. Reopen the app during a session.
- On trusted Wi-Fi, scan the QR with an iPhone. In Safari tap Connect, save files,
  verify bytes, names and empty files. Repeat with Android and a laptop browser.
- Open the invitation in another browser: it must fail after the first claim.
  Try incorrect/missing cookies and an old link after Stop/restart: all must fail.
- Disable shared Wi-Fi, enable the phone hotspot, connect the peer to it and repeat
  with the hotspot's displayed address. Confirm no internet connection is needed.
- Enable uploads. Upload two same-named files and Unicode names, approve each,
  verify unique saved files. Reject another; check it is removed. Try >100 MiB.
- Enable text. Send Unicode, a link and HTML-looking text. Nothing should execute
  or open. Tap Copy and paste into a local editor; verify exact text.
- Interrupt a download and retry. Finish every file in download-only mode and
  verify listener closure. Test a 1-minute expiry, Stop, Wi-Fi loss and hotspot
  shutdown with an active upload; no pending file should survive.
- Kill/restart the daemon with a staged file; verify startup removes it, approved
  files survive, and old tokens fail. Log out and verify user-service shutdown.
- Check the phone firewall on wireless, cellular and USB/Ethernet interfaces;
  only explicitly configured wireless interfaces should admit TCP 8765.
- Verify Safari in private browsing, cookie settings and Files save behavior.
  Browser/OS save dialogs and content-hub activation cannot be certified offscreen.

## Patch series

The supplied workspace marks `.git` read-only. Small commits are therefore
recorded as ordered, binary-capable `git format-patch` files in `patches/`.
Apply them on a clean checkout of the initial task commit using
`git am /path/to/patches/0*.patch`. `patches/series.txt` lists the steps. The working
tree already contains their combined result; do not apply them over it.
