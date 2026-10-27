# Final report — fp6-bugreport

Implemented the task in this repository. The final Rust tests, Qt previews, Nix build and module checks pass. No phone or remote debugging host was accessed; testing used synthetic fixtures and local UI rendering. Network access was limited to obtaining sources and dependencies.

## Delivered

- Rust CLI, collector library, reusable redaction module, and narrowly scoped polkit helper. All 12 requested diagnostic sections are implemented, with bounded commands and explicit unavailable/partial-source status.
- `--since`, `--sections`, `--preview`, optional privileged logs, and capture-now with separately consented screenshots. oFono data uses a typed property allowlist before redaction. Uptime is formatted into days/hours/minutes/seconds to survive conservative number redaction.
- Private `0700` report directories under `~/Documents/Bug reports/`, containing `report.tar.zst` and `summary.txt`, both `0600`. The summary is also inside the archive. Reports have unique directories, are published after successful writing, and never upload automatically.
- Real Qt 5.15 / Lomiri UI Toolkit 1.3 QML app with inclusion descriptions, all section switches, time-window choices, restricted-log opt-in and screenshot consent. Collection runs off the UI thread.
- `package.nix`, `module.nix`, `hardware.fairphone6.bugreport.enable`, on-demand systemd user service, polkit policy, touch-enabled Settings-category desktop entry and SVG icon.
- `README.md` with build/install instructions, exact screenshot API assumption, privacy limitations and a real-phone manual checklist. `DESIGN.md` covers architecture, privilege boundaries, storage lifecycle and quick-settings/power-menu shortcut proposals.
- Three genuine offscreen previews: [overview](previews/overview.png), [sections](previews/sections.png), and [capture consent](previews/capture.png), each 400 × 800 pixels. These were visually inspected. UITK's shape nodes require OpenGL; previews use `QT_QPA_PLATFORM=offscreen` with Mesa software OpenGL, not Qt's incompatible software scene-graph backend.

## Verification

| Check | Result |
|---|---|
| `cargo test --locked -j 1` (default features) | **PASS — 16 tests** |
| `cargo test --locked --all-features` | **PASS — 16 tests** |
| `cargo clippy --locked --all-features --all-targets -- -D warnings` | **PASS** |
| `cargo fmt --check` | **PASS** |
| Full `rustPlatform.buildRustPackage` x86_64 build, GUI enabled | **PASS**, including all 16 release-mode tests in the Nix sandbox |
| NixOS module evaluation | **PASS**, no failed assertions; user service is on demand |
| aarch64 package derivation evaluation | **PASS**; no aarch64 build or phone execution claimed |
| Installed GUI with development QML/plugin paths cleared | **PASS**, rendered an offscreen PNG |
| Installed CLI `--preview` | **PASS**, no collection required |
| Installed helper/policy/desktop inspection | **PASS**, exact helper path, no setuid bit, active-session administrator authentication, inactive/other sessions denied, `X-Lomiri-Touch=true` |
| Final Nix source versus working tree | **PASS**, byte-for-byte match |
| Patch application and reconstructed files | **PASS**, seven patches reconstruct the 33 tracked deliverable files, including this report |

The tests cover sensitive-number formats including +40/07xx/+999, device identifiers, MAC/IP addresses, emails, coordinates and abbreviated GPS fields, SSIDs and secrets; redaction idempotence; oFono field selection; section gating; journal windows; core-list-only collection; helper restrictions; archive contents, permissions, path safety and cleanup; screenshot consent/API success/failure; subprocess timeout/output bounds; and CLI validation. All diagnostic fixtures are synthetic.

Nix checks used `NIX_PATH=nixpkgs=channel:nixos-unstable`. The final successful build command was:

```sh
nix-build --cores 8 --max-jobs 1 \
  -E '(import <nixpkgs> {}).callPackage ./package.nix {}' \
  --out-link result-release
```

Final package: `/nix/store/ycmmm7ifim5kl89w7cs86vz40v00ignx-fp6-bugreport-0.1.0`.
Both `result` and `result-release` point to that package in this workspace. Build outputs are ignored by Git.

## History

The workspace `.git` is read-only, so changes are preserved as a sequential email patch series in `patches/`, ordered by `patches/series`:

1. Bounded Rust collectors, redaction and private archives.
2. Lomiri QML app, Nix packaging and polkit integration.
3. Redaction/modem hardening and Qt/Nix integration fixes.
4. Documentation, manual checks and actual offscreen previews.
5. Abbreviated GPS-field redaction.
6. Readable, redaction-safe uptime.
7. This final report.

The patches were applied to a separate fresh checkout of the original task-brief commit and the resulting files compared with this workspace. To apply them elsewhere, use a clean writable checkout and `git am /path/to/patches/*.patch`. The original repository's Git metadata was not modified.

## Remaining phone-only validation and limits

- The screenshot adapter assumes the session service/interface `org.gnome.Shell.Screenshot`, object `/org/gnome/Shell/Screenshot`, and `Screenshot(b,b,s) → (b,s)`. Its presence on this Lomiri port is **unverified**. Missing/denied API calls produce a text-only report with an unavailable status. The exact contract and rationale are in README.
- Real polkit-agent interaction, shell minimization/restoration, screenshot pixels, hardware sysfs layout, service names and aarch64 runtime behavior need the supplied phone checklist. Nothing was deployed or enabled on this laptop or a phone.
- The delivered Settings entry is a standalone touch app in the Settings/System launcher categories, not an in-process System Settings plugin. Quick-settings and power-menu changes are documented proposals, as requested.
- Text redaction is conservative and can remove useful counters or whole lines. It cannot prove anonymity for arbitrary prose or unusual encodings. Screenshots are explicitly opt-in and unredacted. Users must review reports before sharing; the application has no upload or telemetry feature.
