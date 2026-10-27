# Quick Share for Fairphone 6 / Lomiri

Rust LAN daemon and CLI, with a Qt 5.15 / Lomiri UI Toolkit 1.3 application.
Receives and sends files, text and HTTP(S) links using the Quick Share wire
protocol. Both devices must use the same Wi-Fi LAN. Visibility starts **off**;
the app switch enables it for ten minutes. Always compare the displayed PINs.
See [DESIGN.md](DESIGN.md) for protocol sources, security policy and limitations.

## Build and test

With stable Rust, a C toolchain and `protoc` on PATH:

```sh
cargo build --release
cargo test
# Session API/CLI checks, using the debug binaries:
cargo build
tests/dbus-smoke.sh
```

On NixOS the development shell supplies Rust, protobuf, Qt, CMake and Lomiri:

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell
cargo build --release
cargo test
cmake -S app -B build/app
cmake --build build/app
```

Build the complete installable package, including the QML app:

```sh
nix-build -E 'with import <nixpkgs> {}; callPackage ./package.nix {}'
# The package also runs cargo tests during its check phase.
nix-instantiate --eval --strict tests/module-eval.nix
FP6_BIN_DIR="$PWD/result/bin" tests/dbus-smoke.sh
```

The lockfile pins crate checksums. Protocol dependencies are vendored, with
licenses and exact upstream revisions in [vendor/UPSTREAM.md](vendor/UPSTREAM.md).
No remote protocol service or Google account is needed. Builds fetch sources
and the usual Nix dependencies; the installed app does no telemetry.

## Run

Within your graphical user's session:

```sh
# Set the actual Wi-Fi interface name; do not run as root.
./result/bin/fp6-quickshare-daemon --interfaces wlan0
# In another terminal:
./result/bin/fp6-quickshare status
./result/bin/fp6-quickshare devices
./result/bin/fp6-quickshare receive --visible 10m
./result/bin/fp6-quickshare send ./sample.txt --to 'Android phone'
./result/bin/fp6-quickshare send --text 'Synthetic message' --to 'Android phone'
./result/bin/fp6-quickshare send --url 'https://example.invalid/demo' --to 'Android phone'
./result/bin/fp6-quickshare-app
./result/bin/fp6-quickshare-app file:///absolute/path/to/sample.txt
```

`receive` prints the sender, files and PIN, then requires typing `yes`; Ctrl-C
turns visibility off. It also prints received text. `--visible always` is an
explicit persistent visibility choice for the daemon's current lifetime.
`receive --accept-for-testing` bypasses consent for synthetic local testing
only. `accept ID`, `reject ID` and `cancel ID` aid API debugging.

The GUI uses content-hub to choose files and to accept incoming Share menu
items from other apps. It has receive/send screens, PIN confirmation, progress,
cancel, and Open/Copy actions. Received files are stored under
`~/Downloads/Quick Share/` with private permissions and collision suffixes.
A symlinked Downloads path is rejected; select an absolute ordinary directory
with the daemon's `--download-dir` if needed.

If Android does not appear, open its stock Quick Share **Receive** screen and
choose Everyone visibility. Some Android/vendor versions require BLE wake-up
or another supported receive workflow; BLE is not implemented here. Contacts
visibility is unsupported. See the detailed discovery discussion in DESIGN.md.

## Enable on NixOS

```nix
{
  imports = [ ./quick-share/module.nix ];
  hardware.fairphone6.quickShare = {
    enable = true;
    wifiInterfaces = [ "wlan0" ]; # laptop example: "wlp2s0"
    deviceName = "Fairphone 6";
    # port = 53317;
    # maxBytes = 1073741824;
  };
}
```

Then rebuild the configuration and start/restart the graphical user's service:

```sh
systemctl --user start fp6-quickshare.service
systemctl --user status fp6-quickshare.service
journalctl --user -u fp6-quickshare.service
```

Avahi is enabled for `.local` resolution; transport mDNS is built in. The module
opens UDP 5353 and the configured TCP port **only on the named Wi-Fi interfaces**.
It does not open Ethernet/global ports. Do not enable a separate global firewall
exception. Ensure multicast and client-to-client traffic are allowed by the AP.
The packaged D-Bus activation file uses the same user service when systemd
activation is available. The service restarts with visibility off.

## Offscreen previews

The complete package supplies the QML import paths and native Qt bridge:

```sh
QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1 \
  ./result/bin/fp6-quickshare-app --preview --screenshot /tmp/quickshare-receive.png
QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1 \
  ./result/bin/fp6-quickshare-app --preview-send --screenshot /tmp/quickshare-send.png
```

Preview uses synthetic data and never contacts the daemon. UITK shapes require
an OpenGL context: use Mesa software OpenGL as above, not Qt Quick’s raster
`QT_QUICK_BACKEND=software`, which is incompatible with UITK shapes. To run the app from
a development build use `build/app/fp6-quickshare-app` in `nix-shell`. UI uses
UITK 1.3 margins instead of unsupported Label padding; it needs no GSettings
schemas or integer-array keys.

## Manual Android / Fairphone checklist

Use synthetic files and record Android vendor/version, Play services version,
NixOS revision, Wi-Fi interface and whether the stock receive screen is open.

- Start/restart the daemon. Verify OFF and that Android cannot start a request.
- Enable the ten-minute switch. Share from Android; verify sender, filenames,
  total bytes and matching four-digit PIN. Decline first; verify no file exists.
- Repeat and accept. Compare SHA-256 hashes for a multi-chunk file, multiple
  files, Unicode names and a zero-byte file. Verify 0700 directory/0600 files.
- Send a file again. Verify the original is unchanged and ` (1)` is added.
- Cancel from both ends mid-transfer. Verify status and partial-file cleanup.
- Send text and an HTTP(S) URL in each direction. Confirm consent, notification,
  Copy and Open. An untrusted link must never open by itself.
- Open Android Quick Share Receive/Everyone; discover and send from the FP6.
  Record if BLE absence prevents discovery and any Samsung-specific behavior.
- Wait ten minutes. Verify visibility and advertisement turn off, pending
  requests decline, and previously accepted transfers can complete.
- Share into Quick Share from another Lomiri app via content-hub; choose files
  through the native picker and send them. Test command-line file:// URIs too.
- Test portrait/landscape, notification review/actions, clipboard paste into
  another app, display lock, suspend/resume, Wi-Fi changes and service restart.
- Confirm TCP port and mDNS are reachable on Wi-Fi, closed on Ethernet, and no
  protocol HTTP/internet traffic occurs. Verify AP isolation failure is clear.
- Exercise insufficient space and configured size limits on a disposable test
  volume; verify the original files are preserved and no unintended path opens.

Local loopback tests do not replace this hardware checklist. aarch64 cross-build
and physical FP6 / stock Android interop are still required.

## Commit history in restricted workspaces

If `.git` is read-only, this task's commits are stored in `.commit-history/`.
Inspect them with `git --git-dir=.commit-history --work-tree=. log --oneline`.
Once the normal repository metadata is writable, import them with
`git fetch .commit-history main`, then inspect `FETCH_HEAD` before updating the
normal branch. The working files already contain the implementation.
