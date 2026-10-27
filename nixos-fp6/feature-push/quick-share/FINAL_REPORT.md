# Final report

Implemented a Rust Quick Share LAN core, session D-Bus daemon, debugging CLI,
and native Qt 5.15 / Lomiri UI Toolkit 1.3 application. The protocol engine is
pinned, vendored rqs_lib with security patches and outbound text/URL support.
Sources, licenses, protocol research and remaining limitations are documented
in DESIGN.md; build/run instructions and the Android checklist are in README.md.

## Working and verified locally

- Encrypted TCP file, text and URL transfers between independent protocol
  instances; matching UKEY2 PINs; explicit consent; reject and cancel actions.
- Multi-chunk files, empty files, collision suffixes, private filesystem modes,
  symlink/traversal rejection, size/free-space checks and failed-file cleanup.
- The requested D-Bus properties, methods and signals. A private-bus integration
  test runs the actual daemon and transfers a file through D-Bus acceptance;
  duplicate approval and invalid transfer IDs are rejected.
- Default visibility OFF, ten-minute monotonic expiry, restart OFF, configurable
  device name, independent discovery, and explicit rejection of contacts mode.
- CLI devices/send/receive/status and consent/cancel debugging commands.
- Receive/send QML screens, incoming PIN sheet, progress, cancel, Open/Copy,
  command-line URIs, content-hub picker/share handling, desktop entry and peer
  definition. The two screens render in offscreen previews with synthetic data.
- Nix packaging and a NixOS module with a user service, explicit Wi-Fi interface
  allowlist and interface-specific TCP/mDNS firewall configuration.

## Validation

On x86_64 Linux, using stable Rust 1.96.1 and nixos-unstable
`26.11pre1100280.3ec636362e18`:

| Check | Result |
| --- | --- |
| `cargo build --release` | Passed |
| `cargo test` | Passed: 13 tests, plus doc tests |
| Rust formatting check | Passed |
| `tests/dbus-smoke.sh` against release and Nix-installed binaries | Passed |
| `nix-instantiate --eval --strict tests/module-eval.nix` | Passed |
| CMake Qt application build | Passed |
| `tests/preview.sh` with development and Nix-installed apps | Passed: receive/send PNGs |
| Required `nix-build -E 'with import <nixpkgs> {}; callPackage ./package.nix {}'` | Passed, including release-mode tests and Qt installation |

The Rust tests cover RFC 5869 HKDF-SHA256, mDNS encoding, UKEY2 commitment and
invalid curve-point rejection, bad HMAC/replayed sequence rejection, oversized
frames, payloads before consent, truncated-file cleanup, filesystem policy,
encrypted loopback, daemon D-Bus transfers and the visibility deadline. The
upstream rqs library did not include fixed UKEY2 handshake transcript vectors;
PIN agreement is checked during real encrypted loopback handshakes.

Offscreen preview requires an OpenGL context. `LIBGL_ALWAYS_SOFTWARE=1` works;
Qt Quick's raster `QT_QUICK_BACKEND=software` is incompatible with UITK's shape
renderer. The preview helper sets the working configuration. An upstream UITK
popup deprecation/performance message may still appear without affecting output.
The installed package includes QML imports, Suru assets, the Qt SVG plugin,
desktop/content-hub entries, D-Bus activation and the user unit. The final build
is available through `result`, pointing to
`/nix/store/9w22jbwjs36gdrlhpc5yjcvx7swz5p1y-fp6-quickshare-0.1.0`.

## Physical-device validation still needed

No physical Android phone or Fairphone was available. Do not treat loopback
success as verified stock Android interoperability. Run README.md's checklist
for both transfer directions, real discovery/PIN confirmation, hashes, expiry,
notifications/clipboard, content-hub from another app, Wi-Fi changes and suspend.
The coordinator still needs to perform the aarch64 build and FP6 deployment.

BLE wake-up, contacts/account authentication, QR rendezvous, Wi-Fi Direct/hotspot
upgrade and IPv6 are not implemented. Some stock Android/vendor versions will
not publish mDNS until their Receive screen or another supported wake-up
workflow is active. No companion APK is required by the implemented LAN
protocol, but discovery without BLE is device/version dependent. Abrupt process
termination can leave partial files; sender completion is not a separate remote
disk-durability acknowledgement.

## Commits and workspace restriction

The provided `.git` is mounted read-only, so creating its index lock failed.
Seven implementation commits were recorded in the writable `.commit-history/`
directory while retaining the original starting commit. The final report is
recorded in an additional final commit. The working tree contains all changes.

```sh
git --git-dir=.commit-history --work-tree=. log --oneline
# Once the normal .git directory is writable:
git fetch .commit-history main
# Inspect FETCH_HEAD before updating the normal branch.
```

No system configuration was activated and no real device transfer was attempted.
