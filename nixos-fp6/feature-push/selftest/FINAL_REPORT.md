# Final report — fp6-selftest

Implemented and verified on x86_64. No phone or remote debugging host was accessed; sibling directories were read-only references. No cargo cache or build output is bundled.

## Delivered

- Rust CLI with 20 base checks, 17 data-driven feature entries and two filesystem checks; explicit PASS/WARN/FAIL/SKIP, text/JSON, worst-result exit codes and private report saving.
- Per-check subprocess deadlines, process-group cleanup, bounded output, local Unix D-Bus only, unique-owner calls without activation, read-only probe allowlist and typed reply validation. Raw personal data and errors never enter reports.
- Strict schema dry-run compilation, including inherited `ai`/`ax` rejection, and touch desktop Exec/Icon checks. Known installed systemd units distinguish missing features from stopped/failed ones.
- Qt 5 / Lomiri UI Toolkit 1.3 UI, a Settings “Run self-test” page, desktop/URI integration, and actual offscreen PNGs: [ready](previews/ready.png), [results](previews/results.png). Both are 360 × 688 and use synthetic data.
- `package.nix`, `module.nix`, development shell, private-bus/fixture tests, [README.md](README.md) and [DESIGN.md](DESIGN.md), including the OTA start-check contract.
- Four incremental commits preserved as a binary-capable patch series under [patches/](patches/). Repository `.git` was left untouched; commits were made in temporary Git metadata. The series replays from `bbf0061687f8d6f91a2fd20b034b1e74c91abf57` and reproduces the delivered source files.

## Verification

| Check | Result |
|---|---|
| `cargo test` | PASS — 15 tests, synthetic fixtures and private buses |
| `cargo test --all-features` | PASS — same 15 tests, UI compiled |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo fmt` / patch whitespace checks | PASS |
| Final nixos-unstable `nix-build --no-out-link` | PASS — includes all 15 release-profile tests |
| NixOS module evaluation, x86_64 and aarch64 | PASS — user/system services, timer and MIME assertions |
| Installed CLI smoke | PASS — help, JSON, exit 2, redacted invalid input, 0700 directory and 0600 JSON/text reports |
| Installed Settings/desktop assets | PASS — manifest/component, executable, icon and URI metadata present |
| Installed GUI with development QML/plugin paths unset | PASS — both offscreen previews rendered and visually checked |
| Patch replay and byte comparison | PASS |

The final built package is:

```text
/nix/store/9a4b43g654lp0ahqbjbbv8pvjyzbcnph-fp6-selftest-0.1.0
```

The aarch64 package was **evaluated, not built or run**:

```text
/nix/store/h07kgm1k3qhig3nvb0wv849grpvdwkxp-fp6-selftest-0.1.0.drv
```

`NIX_PATH=nixpkgs=channel:nixos-unstable` was exported as requested. When URL refresh stopped working, final checks used the already cached unstable source explicitly:

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-build -I nixpkgs=/nix/store/zqznszbq59yjk0cm3ray4pfyglaps6qb-source --no-out-link
```

That source is nixpkgs `26.11pre1100280.3ec636362e18`, revision `3ec636362e1816bc2f0888fa0fa2c69f1bc9e504`, Qt 5.15.19 and Rust 1.99.0. Development tests also passed with the installed 26.05 checkout. Build logs and cargo artifacts stayed under `/tmp` or the Nix store. Offscreen rendering uses Mesa OpenGL and Xvfb with `QT_QPA_PLATFORM=offscreen`; UITK shape nodes do not work with Qt Quick's software scene-graph backend.

## Integration and remaining phone work

From the parent `fp6-codex` configuration directory, add exactly:

```nix
imports = [ ./selftest/module.nix ];
hardware.fairphone6.selftest.enable = true;
```

The default user is `phablet`. Set `hardware.fairphone6.selftest.user` and, if needed, `sessionTarget` for the actual phone session. Automatic checks run two minutes after boot and 90 seconds after session activation, including OTA reboots. The module installs `/etc/fp6-selftest/probes.toml`; CLI and UI share it. No shared trial file or updater was modified.

The coordinator must complete these device-specific checks:

- [ ] Build aarch64, import the module, boot and verify both timers and report ownership/permissions.
- [ ] Open Settings → Run self-test; confirm URI dispatch starts the runner once, the app drawer launcher works, and long results scroll correctly.
- [ ] Verify actual display/Lomiri/Maliit, audio defaults, oFono, data-context, GPS and fingerprint contracts on the phone. GPS/fingerprint currently use GeoClue/fprint; substitute adapters for other stacks.
- [ ] Replace seven explicit **unverified** slots: UsbModes1, Update1, notifications/DND policy, battery, app-ready, privacy and gestures. Their real contracts were not supplied by the sibling references. They do not receive guessed status calls, and their absence is not proof that the corresponding feature is uninstalled.
- [ ] Extend the netpolicy helper upstream with a read-only status API if deeper coverage is required. Its existing Sync/SetMobilePreferred methods change state, so the runner only introspects and reports WARN. No sibling was changed.
- [ ] Test airplane mode, missing SIM, disabled data and intentionally stopped features; confirm no activation, discovery, playback, capture or network request occurs.
- [ ] Confirm journal visibility and counts. Inaccessible/empty scopes WARN; counts cap at 1000 records. A permission-limited view is not proof of system-wide health.
- [ ] Wire the updater's local `fp6-updated start check` to `fp6-selftest --quick --json` in the target user/session context, using the policy in DESIGN.md. Decide mandatory checks explicitly; SKIP/WARN must not silently certify an OTA slot.

These integration gaps are visible in reports and configuration. The runner intentionally cannot certify unavailable or unaudited phone APIs. The full manual checklist and coverage limits are in README.md and DESIGN.md.
