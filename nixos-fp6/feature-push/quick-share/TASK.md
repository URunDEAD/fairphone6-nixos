# Task: Quick Share for the Fairphone 6 (NixOS + Lomiri) — Rust

Goal: a Rust utility + native-looking Lomiri app that shares files/text/links
with Android phones using Google's Quick Share (formerly Nearby Share)
protocol over the local network (Wi-Fi LAN: mDNS discovery + TCP, UKEY2
handshake, protobuf frames), so a stock Android phone can send to and receive
from the Fairphone 6 without installing anything.

Context
- Target: Fairphone 6, aarch64 NixOS (Mobile NixOS) running the Lomiri shell
  (Ubuntu Touch UI, Qt 5.15 + Lomiri UI Toolkit 1.3 "Suru" look). Apps are
  QML with optional C++/Rust backends; system services are systemd user units;
  D-Bus is used for IPC.
- Existing open implementations to study and reuse (check licences; GPL-3
  is fine for this project): rquickshare (Martichou/rquickshare, Rust core
  library "rqs_lib" + Tauri UI), NearDrop (Swift, macOS, receive-side
  protocol notes), Google's protocol docs/protos where published.
- Prefer reusing rqs_lib (as a git dependency or vendored fork with minimal
  patches) over reimplementing the protocol, unless it is unmaintained or
  unsuitable; justify the choice in DESIGN.md.

Deliverables (all in this directory, which is a git repo; commit in small
logical steps with clear messages):
1. `quickshare-core` (or a thin wrapper around rqs_lib): receive + send over
   Wi-Fi LAN. Bluetooth/BLE advertising is optional (document what Android
   needs to see the device: Android discovers Quick Share receivers via mDNS
   on the same network when "Everyone/Contacts" visibility; BLE is used to
   wake/visibility on some versions — investigate and document).
2. `fp6-quickshare-daemon`: a systemd *user* service exposing D-Bus API
   `com.fairphone6.QuickShare1`:
   - properties: Visibility (off / everyone-for-10-min / everyone / contacts?),
     DeviceName, State;
   - methods: StartDiscovery/StopDiscovery, Send(target, [uris|text]),
     Accept(transfer_id), Reject(transfer_id), Cancel(transfer_id);
   - signals: DeviceFound/Lost, IncomingRequest(transfer_id, sender, files,
     pin), Progress(transfer_id, bytes, total), Finished(transfer_id, status).
   Received files go to ~/Downloads/Quick Share/ (0600 files, 0700 dir),
   never overwrite (suffix " (1)"), sanitize names, size limit / free-space
   check, reject path traversal. Text/URLs received go to a notification with
   Copy/Open.
3. CLI `fp6-quickshare`: `devices`, `send <file...> [--to NAME]`,
   `receive [--visible 10m]`, `status` — useful for debugging.
4. QML app (Lomiri UI Toolkit 1.3, Suru style, works offscreen for preview):
   - Receive screen: visibility switch ("Visible to nearby devices" with a
     10-minute default), incoming request sheet with sender, file list, PIN,
     Accept/Decline, progress, "Open" when done.
   - Send screen: pick files (accept URIs via command line / content-hub
     share entry), list discovered Android devices, progress, cancel.
   - Desktop entry with X-Lomiri-Touch=true and a content-hub "share" peer
     definition so other apps' Share menus can send to it.
   UITK 1.3 notes: Label has no topPadding/bottomPadding (use margins);
   GSettings keys must NOT use ai/ax types (gsettings-qt aborts).
5. Nix packaging: `package.nix` (rustPlatform.buildRustPackage with a
   Cargo.lock; QML installed to share/) and `module.nix` (NixOS module:
   option `hardware.fairphone6.quickShare.enable`, installs the package,
   the user service, the desktop file, opens the needed firewall ports
   (TCP port range used + mDNS UDP 5353) ONLY on Wi-Fi interfaces, avahi/mDNS
   requirement documented).
6. Tests: unit tests for framing/crypto handshake vectors where available,
   filename sanitising, overwrite rules; an end-to-end loopback test (two
   instances on localhost / network namespace sending a file to each other)
   in `cargo test` or a script. If interop vectors from rqs_lib exist, use
   them.
7. DESIGN.md: protocol overview, security model (UKEY2, PIN confirmation,
   visibility default OFF, auto-off after 10 minutes, never accept without
   user confirmation), threat notes, what was reused, known gaps
   (e.g. BLE wake-up, Wi-Fi Direct/hotspot upgrade not implemented).
   README.md: build, run, test, and a manual interop test checklist with a
   real Android phone.

Constraints
- Rust stable; keep unsafe out. No telemetry. Default visibility OFF.
- Never accept transfers without explicit user confirmation (except an
  explicit CLI flag for testing).
- Don't contact any network service except what's needed to fetch crates and
  sources (crates.io, github.com).
- Keep personal data out of tests (synthetic device names).
- Make sure `cargo build --release`, `cargo test` and `nix-build -E
  'with import <nixpkgs> {}; callPackage ./package.nix {}'` (nixpkgs from
  channel:nixos-unstable is available as <nixpkgs> via NIX_PATH if you
  export NIX_PATH=nixpkgs=channel:nixos-unstable) succeed on this x86_64
  laptop. aarch64 cross-builds will be done later by the coordinator.

When done, write FINAL_REPORT.md summarising what works, what was tested,
and what needs testing on real hardware.
