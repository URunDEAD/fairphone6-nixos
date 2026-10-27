# Final report

Implemented the requested local-network sharing app using the task's **HTTP-on-LAN**
option. The working tree contains the Rust service/CLI, Lomiri QML app, content-hub
share target, browser page, Nix package/module, tests, documentation, and previews.
No phone was contacted or deployed to. Test and preview data are synthetic.

## Delivered behavior

- Wi-Fi/hotspot-only listener: kernel wireless-interface discovery, private IPv4,
  SO_BINDTODEVICE, explicit address binding, and same-subnet peer checks.
- Fresh 128-bit invitation in a short URL and locally generated QR. An explicit
  browser Connect action consumes it once and issues a separate HttpOnly,
  SameSite cookie; one browser can download multiple selected files.
- Only explicitly selected regular files are available, through numeric IDs.
  No general directory listing, remote path selection, or uploaded-file serving.
- Uploads default off: streaming size/time/concurrency limits, private staging,
  individual phone approval, rejection/expiry cleanup, unique no-clobber saves.
- Text defaults off: bounded in-memory queue and an explicit phone-side Copy
  action. Text/links are never executed or opened automatically.
- Automatic completion/expiry/network-loss shutdown, request and connection
  limits, host/origin validation, attachment downloads, and defensive headers.
- `com.fairphone6.WebShare1` session-bus API, `fp6-webshare` CLI, authenticated local
  QML control adapter, content-hub file picker and Share via link target.
- `hardware.fairphone6.webshare.enable`, systemd **user** service, touch desktop
  entry, and firewall exceptions restricted to explicitly configured interfaces.
- `README.md` contains installation, CLI/API usage, limits and a real-phone
  checklist. `DESIGN.md` explains trust boundaries and completion semantics.

## Verification

All final checks below passed on this x86_64 Linux laptop.

| Check | Result |
| --- | --- |
| `cargo test --locked` | 18 integration/security tests passed |
| `cargo fmt --check` / formatting | Source formatted with rustfmt |
| `cargo clippy --all-targets -- -D warnings` | Passed, no warnings |
| `scripts/dbus-smoke.sh` | Bus lifecycle, duplicate-daemon rejection, private control file, invalid-interface rejection and stale-staging cleanup passed |
| `nix-build --no-out-link` | Package built successfully; all 18 tests passed again in the Nix sandbox |
| Installed package CLI | `fp6-webshare --version` returned `0.1.0`; desktop entry and wrapper inspected |
| `nix/module-check.nix` | Enable/disable behavior, interface-scoped firewall and content-hub installation checks passed; aarch64 cross derivation evaluated |
| Offscreen QML previews | Setup, sharing and approval states rendered and visually inspected |
| Independent QR decoding | ZBar decoded the sharing screenshot to the expected synthetic URL |
| Patch replay | First five patches applied cleanly to the task baseline and produced an identical Git tree |

The Rust tests cover one-use concurrent invitation claims, authenticated downloads,
completion and interrupted-download retry, expiry, disabled receiving, upload
approval/no-overwrite, traversal and unsafe names, byte limits with and without
Content-Length, queue/rate limits, rejection/stop cleanup, bounded text, foreign
origins/hosts/subnets, HTML escaping, symlink rejection, pinned file handles,
transport response draining, and control API authentication.

Build environment: Rust/Cargo 1.99.0 stable, Qt 5.15.19, Lomiri UI Toolkit 1.3.5909.
Nix checks used `NIX_PATH=nixpkgs=channel:nixos-unstable`.
Built package:
`/nix/store/9y7xyzqpdnpsl69945mzj0jfvcf0zc18-fp6-webshare-0.1.0`.

Previews: [setup](previews/idle.png), [sharing](previews/sharing.png),
[approval](previews/approval.png). The reproducible preview script retains
`QT_QPA_PLATFORM=offscreen` and uses Mesa software OpenGL under Xvfb. QtQuick's
non-OpenGL `software` backend crashed with this UITK; the final preview path
renders successfully without substituting another UI.

## Scope and remaining device checks

HTTP is **not encrypted**; this choice is stated in the app, CLI, web page and
README. HTTPS and certificate fingerprints are not implemented because HTTP-on-LAN
was an expressly permitted alternative. Use trusted Wi-Fi or a private hotspot.

A native aarch64 build, real Fairphone Wi-Fi/hotspot behavior, Lomiri app-launch /
content-hub activation, Maliit and clipboard integration, and actual iPhone Safari
save dialogs still need the README's device checklist. Cross evaluation and
laptop previews do not establish those hardware results. The task prohibited
accessing a phone or remote host, so none was attempted.

Current deliberate limits include IPv4-only wireless networks, no range/resume,
and a single receiving browser. Download-only sessions stop after all files have
streamed. Receive-only sessions stop after the first approved file or consumed
text, discarding other unapproved items; mixed sessions stop on Finish, Stop,
network loss or expiry. The local control daemon remains available after the
LAN listener closes. The normal Nix firewall remains an additional boundary.

## Change history

The workspace marks `.git` read-only. Six small commits are preserved as an
ordered, binary-capable patch series in `patches/`; the original Git metadata was
not modified. `patches/series.txt` lists the commits. Patches 1–5 contain the
implementation and verification assets; patch 6 contains this report.

Apply to a clean checkout of baseline `4d81fd2`:

```sh
git am /path/to/patches/0*.patch
```

The working tree already contains the finished result. Do not apply the patches
over these same files.
