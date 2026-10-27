# Final report

Implemented the task in this repository. The deliverable includes:

- Stable Rust policy engine, `netpolicyd`, privileged `netpolicy-helper`,
  `netpolicyctl`, and the optional Qt 5 `netpolicy-ui` binary.
- Foreground-aware data saver, configurable systemd scope matching, per-app
  background exceptions, nftables RX/TX counters, UTC monthly usage and
  warning/background-limit settings.
- NetworkManager observation, interface-bound gateway/public probes, capped-plan
  consent, hysteresis, route ownership/expiry handling and notifications.
- Three Lomiri UITK 1.3 settings pages and actual 360 × 720 offscreen renders in
  [previews/](previews/). No invented app usage appears in the previews.
- `rustPlatform.buildRustPackage` packaging, a
  `hardware.fairphone6.netpolicy.enable` NixOS module, user/system services,
  restricted system D-Bus policy, and an `X-Lomiri-Touch=true` desktop entry.
- [DESIGN.md](DESIGN.md), [README.md](README.md), integration scripts, golden
  fixtures, MIT license and the coordinator's manual phone checklist.

## Verification

| Check | Result |
| --- | --- |
| `cargo test --locked` on x86_64 | Passed: 21 policy, parsing, usage, rule-generation and discovery tests |
| Private-bus integration | Passed: 2 additional tests, including real helper UID authorization and denied mobile activation |
| Rootless network namespace | Passed: IPv4/IPv6 allow, block, restore, RX/TX accounting and counter preservation; no root fallback needed |
| `cargo fmt --all --check` | Passed |
| Clippy, all targets and UI feature, warnings denied | Passed |
| `cargo build --features ui` | Passed |
| `QT_QPA_PLATFORM=offscreen` preview script | Passed for settings, usage and thresholds with actual Qt 5/Lomiri rendering |
| Installed Nix UI launcher | Passed an additional offscreen render with development QML/plugin paths unset; packaged icons and imports resolved |
| `nix-build --no-out-link` | Passed, including release-mode Rust tests with the UI feature |
| NixOS module evaluation | Passed for x86_64-linux and aarch64-linux |
| Module defaults | Confirmed graphical-session-pre ordering and ModemManager disabled by default for the oFono target |
| Patch replay | Verified in a clean clone; reconstructed source tree matches the recorded history |

The 2 private-bus tests are intentionally ignored by ordinary `cargo test` and
were explicitly run with `scripts/dbus-test.sh`. The namespace test used synthetic
HTTP data and disposable veth links. No host firewall changes or real network
probes were made during testing.

The final x86_64 Nix package is:

```text
/nix/store/3hgqhd4f432jvaar2xfml4lwrj53zdyg-fp6-netpolicy-0.1.0
```

Nix checks used `NIX_PATH=nixpkgs=channel:nixos-unstable`. A writable temporary
`XDG_CACHE_HOME` was necessary in this restricted environment. The preview setup
uses Mesa software OpenGL and Xvfb while retaining `QT_QPA_PLATFORM=offscreen`;
UITK's shape renderer cannot use Qt Quick's software scenegraph backend. Final
previews render without QML warnings.

## Coordinator handoff and practical limits

The intentionally separate oFono adapter must implement the documented
`status`, `prefer-mobile`, and `prefer-wifi` contract, including a stable/reserved
interface and persistent ownership/rollback. With no adapter, automatic mobile
switching remains unavailable. Lomiri must provide configured app IDs and renew
foreground focus through D-Bus; the quick-settings tile contract is documented.
Actual app scope patterns and VPN/push exceptions must match the phone's units.

These are sampled policies and usage estimates, not byte-exact carrier quotas.
Updates occur on reconciliation, and quotas can overshoot within a sampling
interval; foreground traffic always remains permitted. Tethered traffic,
no-socket/kernel traffic and shared proxies have attribution limits. VPNs are
handled conservatively when chargeable uplinks coexist. The full operational
semantics, restart behavior and limits are in DESIGN.md.

No phone or remote host was accessed beyond documentation/source/dependency
fetching. The aarch64 module was evaluated, but a complete aarch64 image was not
built or run. On-device oFono routing, focus integration, VPN behavior, kernel
support and Maliit interaction remain the coordinator's manual verification.
Automatic Wi-Fi enabling near saved networks is explicitly out of scope.

## Commit substitute

The repository's `.git` is read-only (`git add` could not create `index.lock`).
Work was recorded in a separate temporary Git history and exported as **8 ordered
mail patches** under [patches/](patches/); [patches/series](patches/series) lists
application order. The final patch adds this report. The base commit is:

```text
8556230600dd09fc0cf59dd48c1a54d46a3b857f
```

Apply with `git am /path/to/patches/*.patch` in a clean clone at that base, not over
this already populated working tree. The original `.git` was left unchanged.
