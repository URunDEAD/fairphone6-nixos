# KDE Connect for Lomiri

A Rust user daemon and Qt 5.15 / Lomiri UI Toolkit 1.3 app for the Fairphone 6
running Mobile NixOS. Connect to KDE Connect on KDE/Android or GSConnect on a
shared Wi-Fi network. This is an initial implementation: automated checks use
synthetic peers; interoperability with real devices still needs the checklist
below. No phone or remote host was accessed during development.

Pairing requires a local action. Compare the peer SHA-256 certificate fingerprint
and, for protocol 8, the eight-character verification code on both screens.
Requests expire after 30 seconds. The saved certificate is pinned; a changed
certificate or a protocol downgrade fails closed. To replace a peer's keys,
unpair it locally first and pair again.

Only ping and battery are enabled after pairing. All other plugins require an
explicit per-device toggle. Remote input additionally requires module-level
uinput permission, lasts at most ten minutes, and is revoked on disconnect or
restart. There is no telemetry, cloud account, address-book access or cellular
transport.

## Build and run on the laptop

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell --run 'cargo test'
nix-shell --run 'cargo build --features ui'
nix-build
nix-instantiate --eval --strict --expr 'import ./tests/module.nix {}'
./scripts/preview.sh
```

The default Cargo build contains the daemon and CLI without Qt. `--features ui`
builds the Rust Qt bridge and QML application. `shell.nix` supplies stable Rust,
OpenSSL, Qt, UITK and an isolated Cargo cache under `/tmp`. `package.nix` uses
`rustPlatform.buildRustPackage`, the committed lockfile, and runs the tests with
an isolated test D-Bus. No tests discover actual devices or inject real input.

For a local development daemon, explicitly supply a Wi-Fi interface:

```sh
./target/debug/lomiri-kdeconnectd --interface wlp1s0
./target/debug/kdeconnectctl devices
```

Run only one KDE Connect implementation on a given host/port. The daemon reports
port conflicts. UDP 1716 is mandatory; this daemon listens for the main TCP
channel on 1716 and sends payloads on an available TCP port in 1739–1764. Peers
may advertise a main or payload port anywhere in 1716–1764.

## NixOS on the phone

Import the module into the phone's existing aarch64 NixOS configuration:

```nix
{
  imports = [ ./kdeconnect/module.nix ];
  hardware.fairphone6.kdeconnect = {
    enable = true;
    user = "your-phone-user";
    wifiInterfaces = [ "wlan0" ];
    # Optional: exposes /dev/uinput to this user's dedicated group.
    # Each device must still receive temporary consent in the app.
    allowUinput = false;
  };
}
```

The module installs a touch desktop entry with `X-Lomiri-Touch=true`, a Content
Hub share target under `share/lomiri-content-hub/peers` (native registry filename
without a `.json` suffix), and two **systemd user** services, scoped to that login user
and `graphical-session.target`. The second service is a small Qt clipboard agent;
it does not read the clipboard until an opted-in paired peer is online. The
Lomiri session must export its graphical environment to the systemd user manager
(including the Qt platform/Mir variables and session bus). It must also provide
its normal freedesktop notification service, Content Hub and PipeWire session.
The UI wrapper supplies a default `APP_ID` while preserving any identity set by
the Lomiri launcher. Restart the graphical session after first installation if
Content Hub started before the new peer directory existed.

The module opens **UDP 1716 and TCP 1716–1764 only on the explicitly named Wi-Fi
interfaces**, with no global firewall ports. It rejects wildcard/cellular/
Ethernet names. The daemon checks the Linux wireless sysfs entry, binds TCP and
outgoing sockets to Wi-Fi IPv4 addresses, and accepts only same-subnet discovery.
It refreshes listeners as addresses appear/disappear. Standalone UDP uses a
wildcard receive socket with source-subnet filtering; use the module firewall
for ingress-interface enforcement. IPv6, mDNS, routed peers and manual remote
hosts are not implemented. Wi-Fi client isolation can prevent discovery.

After enabling uinput access, log out/in for group membership to take effect.
Do not grant access to the general `input` group: the dedicated group only
permits `/dev/uinput`, not reading physical keyboard devices.

## Features and limits

| Feature | Behavior |
| --- | --- |
| Discovery / pairing | UDP broadcast, TCP/TLS 1.2+, v7/v8; pinned self-signed RSA certificate; explicit pairing dialog and fingerprint |
| Notifications | New phone notifications forwarded by a session-bus monitor; remote notifications displayed using `org.freedesktop.Notifications`; update/cancel support; origin hint prevents loops |
| Clipboard | Opt-in text in both directions through Qt's native clipboard bridge, maximum 16 KiB; RAM only, cleared when no recipient is allowed |
| File share | Authenticated auxiliary TLS, maximum 256 MiB/file, two transfers at once, five-minute transfer deadline; receive under `~/Downloads/KDE Connect` with unique names and no overwrites |
| URL / text share | Remote HTTP(S) links and text shown as notifications/status, never opened automatically; send URLs/local files from Content Hub, desktop share action or CLI |
| Battery | Reports sysfs battery charge/charging/low threshold; shows peer battery |
| Ping | Sends ping; incoming ping shows a notification |
| Find phone | Plays a generated 20-second tone through PipeWire; UI/CLI can stop it |
| Media | Exposes phone MPRIS players, state, transport actions, seek, position and volume; refreshes every five seconds |
| Remote input | Temporary uinput pointer/click/scroll and US ASCII keyboard/special keys/modifiers; no arbitrary commands; no persistent grants |

Notification forwarding depends on the session bus allowing `BecomeMonitor`.
Failure is reported in the app; the daemon never replaces the shell notification
service. Existing notification history, notification icons/payloads, actions and
inline replies are not exported. Notification dismissals are synchronized when
the shell emits the standard close signal. The first version handles one item
per Content Hub share request; select another item for another transfer. Files
are stored with a UUID prefix. Shared links remain in the current in-memory
status until a later event. Non-ASCII typing and pointer drag holds are not
implemented. The keyboard layout is US, independent of the phone's configured
layout. Media album art, remote player browsing in the phone UI, IPv6 and mDNS
are outside this version. There is no lock-screen input integration: keep remote
input disabled when the phone is unattended, and stop the service before locking
if an active grant is no longer appropriate.

## D-Bus and CLI

The session service, interface and object path are:

* service/interface: `com.fairphone6.KdeConnect1`
* path: `/com/fairphone6/KdeConnect1`

| Method | Inputs | Return |
| --- | --- | --- |
| `Devices` | none | JSON string with local identity, devices, fingerprints, pending requests, plugin settings and current status |
| `Discover` | none | none |
| `RequestPair`, `AcceptPair` | device ID and displayed fingerprint strings | none |
| `Unpair` | device ID string | none |
| `SetPlugin` | ID, plugin string, enabled boolean | none |
| `Action` | ID, action string, data string | none |
| `ClipboardState` | none | JSON string with enabled, content and revision |
| `ReportClipboard` | text string | none |
| `StopRinging` | none | none |

Pairing commands must include the exact fingerprint just displayed, so a stale
dialog cannot approve a peer whose key changed during reconnection.
Failures are D-Bus errors. The UI reads snapshots once per second via a worker
thread, so a slow or unavailable service does not block the Qt event loop.
The session bus is the same-user trust boundary; any process running as the user
can invoke these controls, as it can access that user's files and clipboard.

```sh
kdeconnectctl pair DEVICE_ID DISPLAYED_FINGERPRINT
kdeconnectctl accept DEVICE_ID DISPLAYED_FINGERPRINT
kdeconnectctl plugin DEVICE_ID notifications on
kdeconnectctl plugin DEVICE_ID clipboard on
kdeconnectctl plugin DEVICE_ID share on
kdeconnectctl ping DEVICE_ID
kdeconnectctl share DEVICE_ID https://example.org
kdeconnectctl share DEVICE_ID ./example.txt
kdeconnectctl media DEVICE_ID '{"requestPlayerList":true}'
kdeconnectctl stop-ringing
kdeconnectctl unpair DEVICE_ID
```

Plugin names: `ping`, `battery`, `notifications`, `clipboard`, `share`,
`findmyphone`, `media`, `input`. Input permission is reset at daemon startup and
disconnect. Trust and preferences live in
`${XDG_STATE_HOME:-~/.local/state}/lomiri-kdeconnect/`; the directory is mode 0700
and certificate, private key and state files are mode 0600. Clipboard and
notification content are never written to the state file. Downloads are private
through the service's 0077 umask. Unpairing removes trust and preferences; received
files remain under the user's control.

## UI previews

`previews/devices.png`, `previews/plugins.png`, and `previews/pairing.png` contain
synthetic devices. Generate them with `scripts/preview.sh` after `nix-build`.
The script sets `QT_QPA_PLATFORM=offscreen`, Mesa llvmpipe software OpenGL and an
isolated cache. Preview mode does not connect to the daemon or discover peers. UITK shapes need
an OpenGL context, so the Qt software scene graph cannot be used; the offscreen
Qt platform obtains its GLX context from a local X display (use Xvfb in headless
CI).

## Manual checklist for the real phone

These checks are **pending**; perform them with consenting test devices and
non-sensitive data on a trusted Wi-Fi network.

- [ ] Build natively for aarch64 or through an aarch64 Nix builder; enable the
  module and verify the launcher, Maliit fields and Suru controls in Lomiri.
- [ ] Confirm both user services start in the graphical session; stop/restart
  them and check `journalctl --user -u lomiri-kdeconnect` for errors.
- [ ] Discover KDE Connect KDE, GSConnect and Android, one at a time and
  concurrently. Test discovery after Wi-Fi disconnect/reconnect and DHCP changes.
- [ ] Request pairing from each side, compare both code and fingerprint, accept,
  reject and let requests time out. Confirm plugin traffic cannot arrive first.
- [ ] Restart services and peers: trust survives; input permission does not.
  Change a synthetic peer's certificate and attempt a v8-to-v7 downgrade; both
  must be rejected until a deliberate local unpair/new pairing.
- [ ] Verify clipboard/notification/share/media/ring/input default to off for a
  newly paired peer. Toggle each device separately and verify immediate revocation.
- [ ] Enable notifications and create/update/dismiss synthetic notifications on
  both sides. Verify there is no echo loop or notification history replay.
- [ ] Enable clipboard, copy/paste text each direction, test reconnection and
  disabling, and confirm no clipboard reads/updates while no opted-in peer is
  online. Confirm Qt clipboard access in the headless graphical-session agent.
- [ ] Receive duplicate filenames, empty files, a 1 MiB synthetic file, bad paths,
  a truncated transfer and an untrusted payload certificate. Check partial cleanup,
  no overwrites, private permissions and download location.
- [ ] Share a local file and an HTTPS link through Content Hub and CLI. Check the
  transfer remains readable after the sending app releases its Content Hub item.
- [ ] Verify sysfs battery information, incoming/outgoing ping and notification
  presentation with long synthetic text and inert markup.
- [ ] Enable ring; test PipeWire output, UI stop, repeated request and 20-second
  automatic end without leaving a playback process behind.
- [ ] Enable media; start a phone MPRIS player, verify list/state, play/pause,
  next/previous, seek, position and volume from the desktop.
- [ ] With module uinput disabled, input opt-in must fail visibly. With access
  enabled, test pointer/click/scroll, US ASCII/special keys and modifiers. Verify
  `/dev/uinput` virtual-device teardown on disable, ten-minute expiry, disconnect
  and daemon restart; confirm no stuck keys. Disable it before locking the phone.
- [ ] Inspect interface-specific firewall rules; check cellular/Ethernet cannot
  discover/connect to the service. Confirm no outbound advertisements on them.
- [ ] Verify behavior after suspend/resume, screen blank, shell restart and
  peer disappearance. Test on Android and GSConnect versions in actual use.

See [DESIGN.md](DESIGN.md) for the protocol study and security architecture.
