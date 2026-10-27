# Final report

## Outcome and scope

Delivered an experimental Rust Calls1 broker and client, a synthetic incoming-call/demo workflow, a real read-only oFono observer, call/DND/focus policy, ephemeral call history, a Lomiri QML prototype with offscreen PNGs, Nix packaging/module, research/design and a phone acceptance plan.

**This is not a completed hardware CallKit replacement.** The repository contained only the task brief, so actual fp6-notifyd, shell, audio/haptic/sensor and history integrations could not be verified against project code. Their contracts and responsibilities are documented and tested with synthetic services. The native lock-screen/Telepathy bridge, physical ringtone/vibration, PipeWire route implementation, proximity sensor/display integration and native history writer remain phone-port work. The module does not claim these capabilities or install a fake production adapter. A full Telepathy CM and Android bridge are not implemented.

## Delivered files

- `src/model.rs`: Ringing/Dialing/Active/Held/Ended lifecycle, acknowledged actions, focus reservations, cellular precedence, timeout/disconnect cleanup, resource limits, route/mute/proximity effects, answered versus unanswered history.
- `src/service.rs`, `src/ofono.rs`: owned session D-Bus API, controller role checks, redacted snapshots, DND fail-silent behavior, leased desired effects, real cellular snapshot/signal observation and recovery.
- `src/client.rs`, `src/bin/demo.rs`, `examples/synthetic-shell.rs`, `scripts/demo.sh`: native client, `fp6-voip-demo`, and an isolated end-to-end synthetic workflow.
- `qml/`: incoming/active/held/waiting/DND screen, accessible control intents, interactive offline preview, QtQuick tests.
- `previews/`: five 400×800 PNGs rendered using real Qt 5.15 and Lomiri UI Toolkit 1.3 with `QT_QPA_PLATFORM=offscreen`. No image generation or toolkit stub was used.
- `package.nix`, `default.nix`, `module.nix`, `shell.nix`, `data/`: Rust package, `hardware.fairphone6.calls.enable`, systemd user service, touch-enabled preview desktop entry and development environment.
- `DESIGN.md`: Apple/Android/Telepathy research, source-linked Lomiri findings, architecture decision, explicit integration/security assumptions and Waydroid phase 2 design.
- `docs/DBUS_API.md`, `docs/PHONE_TEST_PLAN.md`, `README.md`, `LICENSE`: API, acceptance gates, build/use/manual checklist and MIT license.

## Validation

- `cargo test --locked`: **20 passed** (17 policy tests, 3 private-bus integration tests). Integration tests execute the real daemon and cover DND changes/failure/timeout, owner isolation, controls, effects, privacy, history, provider disconnect, oFono signals/preemption and owner loss. Both session/system buses are private fixtures; no phone bus is contacted.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `scripts/demo.sh`: passed; synthetic incoming → Answer → Mute → Speaker → Hold → Resume → Bluetooth → End, with effects and history.
- `scripts/previews.sh`: passed for incoming, silent, waiting, active and held. Incoming, active and waiting PNGs were visually inspected; active controls were corrected to avoid overlap.
- QtQuick tests: **3 behavioral tests passed**, reported as 5 passes including setup/cleanup. Answer emits an intent without changing state, cellular conflict disables Answer while allowing Decline, and a pending active call can still End.
- `nix-build --no-out-link`: passed with all 20 release-mode tests inside the Nix sandbox. Output: `/nix/store/wd6f7c2lw8v3539jm6j6v0dkax0m38xd-fp6-callkit-0.1.0`.
- Installed `fp6-calls-preview`: passed outside `nix-shell` with inherited QML/plugin paths removed; rendered a 400×800 PNG with the offscreen/software backend.
- `tests/module-eval.nix`: passed; D-Bus user-service settings and session target evaluated. The aarch64 package derivation also evaluated successfully; it was not built or run.
- Patch replay: all implementation patches applied to a fresh copy of baseline `b3a2947` and matched the workspace byte-for-byte. The final report is the eighth patch.

Environment: x86_64 Linux; Cargo/Rust 1.99.0; Qt 5.15.19; Lomiri UI Toolkit 1.3.5909; zbus 5.19.0. Nix checks use `NIX_PATH=nixpkgs=channel:nixos-unstable`. Cargo dependencies are locked; the nixpkgs channel is not pinned. No aarch64 compilation or real-phone tests were performed.

The Nix sandbox initially exposed an implicit dependency on `/etc/dbus-1/session.conf`; tests now ship a minimal private configuration with no activation directories. The UI initially exposed UITK's OpenGL-only Button shape; software-safe QtQuick button geometry retains real UITK labels/units. The Qt launcher uses the actual `qtdeclarative` development output that contains qmlscene. An installed-launcher smoke check also identified QML modules in the separate binary output; the wrapper now includes those runtime module/plugin paths explicitly instead of relying on the development shell.

## Patch history and privacy

`.git` rejected writes with “Read-only file system.” Small commits were created in an isolated temporary Git directory and exported under `patches/`. The original repository's Git metadata is unchanged; the final files are already in this workspace. Apply patches only to a clean task-baseline checkout, following `patches/series` or README instructions.

No telemetry, contact/credential access, audio recording, persistent caller database, phone deployment or remote-host login was performed. Network access was limited to documentation, source and dependency/toolchain fetching. All test/demo data is synthetic. History is bounded, volatile and excludes caller names. Same-user well-known-name ownership is explicitly a cooperative trust model; production app confinement/portal authorization remains required.

## Remaining acceptance gates

1. Match the assumed ShouldAlert shim to the actual fp6-notifyd API and visual/DND exception policy.
2. Implement and validate the preferred single Telepathy bridge, or embed the supplied surface in the trusted Lomiri shell, reserving privileged roles against untrusted apps.
3. Implement the leased platform adapter with actual hardware routing/haptics/sensor/display APIs and crash watchdogs; verify `platform_ready` truthfully reflects applied capabilities.
4. Connect native history exactly once, with agreed persistence/retention/deletion behavior.
5. Build/test on aarch64 and run the phone checklist, including locked UI, Bluetooth duplex, cellular conflicts, DND changes, proximity and crash recovery.

The supplied code and previews establish a testable broker/UI prototype. Laptop mocks do not establish physical phone behavior or security isolation for unrestricted same-UID applications.
