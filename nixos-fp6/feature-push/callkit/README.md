# Fairphone Internet Calls

A Rust prototype of the `com.fairphone6.Calls1` session service for native VoIP apps on Fairphone 6 / Mobile NixOS / Lomiri. Includes a typed client, incoming-call demo, call-state policy, read-only oFono observer, DND and platform integration contracts, volatile call history, and a UITK 1.3 QML screen prototype.

**Phone integration is not complete:** the real fp6-notifyd shim, trusted shell/Telepathy bridge, PipeWire/haptic/proximity adapters and native history writer must be connected to the actual phone stack. Without these, the broker runs but does not display over the lock screen, ring, vibrate, switch physical audio or blank the screen. `platform_ready` remains false without a platform adapter. Nothing in the installed module enables simulated cellular-idle mode. See [DESIGN.md](DESIGN.md) for the researched recommendation and explicit assumptions, and [DBUS_API.md](docs/DBUS_API.md) for interfaces.

![Incoming call preview](previews/incoming.png)

## Build and check on the laptop

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
# Only needed if the normal Cargo cache is read-only:
export CARGO_HOME=/tmp/fp6-cargo
nix-shell
cargo test --locked
cargo fmt --check
cargo clippy --all-targets -- -D warnings
scripts/demo.sh
scripts/previews.sh
QT_QPA_PLATFORM=offscreen QT_QPA_OFFSCREEN_NO_GLX=1 \
  QT_QUICK_BACKEND=software QT_STYLE_OVERRIDE=Fusion GRID_UNIT_PX=8 \
  qmltestrunner -input qml/tests -platform offscreen
exit
nix-build --no-out-link
nix-instantiate --eval --strict --json tests/module-eval.nix
```

With Rust stable and `dbus-daemon` already on PATH, plain `cargo test --locked` works without Nix or a running user bus. Tests spawn isolated private buses, including a synthetic oFono system bus; they never access a phone or your existing system/session bus. The service uses no application `unsafe` code. Cargo.lock pins all dependencies. Nix builds use `rustPlatform.buildRustPackage`, the lockfile, offline vendoring and enabled tests; the nixpkgs channel itself is not pinned. Tested tool versions and results are recorded in [FINAL_REPORT.md](FINAL_REPORT.md).

`nix-build` is a local x86_64 package check. The Nix expression supports Linux/aarch64, but no aarch64 executable or phone runtime has been validated. For an aarch64 build, use this package from an aarch64 NixOS configuration or a deliberately configured cross-compilation environment; the package does not configure a remote builder.

## Run the synthetic demo

Inside `nix-shell`, `scripts/demo.sh` builds everything and starts a private bus, `fp6-callsd --demo`, a synthetic shell/DND/platform fixture, and `fp6-voip-demo`. It exercises Answer, Mute, Speaker, Hold, Resume, Bluetooth and End, printing desired effects and a synthetic history record. It exits and cleans up its child processes. There is no media connection or physical sound/vibration. The `--demo` option assumes cellular Idle and is solely for isolated tests.

For a provider, keep one D-Bus connection alive throughout the call:

```rust,no_run
use fp6_callkit::client::CallsProxy;
# async fn example() -> zbus::Result<()> {
let connection = zbus::Connection::session().await?;
let calls = CallsProxy::new(&connection).await?;
let id = calls.report_incoming("org.example.MyVoip", "Synthetic caller").await?;
// Read Commands/Calls on Changed, apply commands to your media/signalling engine,
// then Complete(id, serial, success). Stop media on Ended or broker disconnect.
calls.report_ended(id).await?;
# Ok(()) }
```

Outgoing providers use ReportOutgoing before starting signalling and ReportConnected when connected. A rejected ReportOutgoing must not start media. Call IDs are local to the current broker instance. See `src/client.rs`, `src/bin/demo.rs` and `examples/synthetic-shell.rs` for working code. The demo provider only simulates acknowledgements; real providers must apply actions before acknowledging them and reconcile uncertain results.

## UI prototype and previews

Run `nix-shell --run 'qmlscene qml/Preview.qml'` for the interactive synthetic screen. `IncomingCall.qml` is an embeddable surface with properties and action signals; `Preview.qml` supplies local synthetic transitions. It has no D-Bus connection and no lock-screen privileges. Its fixed caller text deliberately hides caller identity.

`scripts/previews.sh` renders 400×800 PNGs with `QT_QPA_PLATFORM=offscreen`: [incoming](previews/incoming.png), [DND](previews/silent.png), [cellular waiting](previews/waiting.png), [active](previews/active.png), [held](previews/held.png). The script sets software rendering, a short private runtime directory and an eight-pixel grid unit. UITK's default Button uses an OpenGL-only shape that crashes with the software scene graph; `CallButton.qml` uses QtQuick geometry plus real UITK Label/units instead. No toolkit stub or screenshot fabrication is used. An offscreen OpenGL-context warning can appear despite successful software rendering.

There are no GSettings keys or unsupported Label padding properties. The phone controller should map actionRequested/routeRequested into Calls1 Control, wait for confirmed snapshots, present failures/pending states, and decide whether to embed this surface or reuse the existing Telepathy approver. Maliit is not needed for the call controls; they have no text entry.

## NixOS integration

```nix
{
  imports = [ ./path/to/callkit/module.nix ];
  hardware.fairphone6.calls.enable = true;
  # Change only if your Lomiri session actually starts a different target:
  # hardware.fairphone6.calls.sessionTarget = "lomiri-session.target";
}
```

The module installs a `fp6-calls` systemd **user** service with D-Bus name `com.fairphone6.Calls1`, lifecycle tied to the session target, restart-on-failure, and AF_UNIX-only socket access. It installs a desktop entry with `X-Lomiri-Touch=true` that launches **Internet Calls Preview**. That entry is a synthetic screen, not a production incoming-call handler. There is no root service, automatic D-Bus activation, firewall change, phone deployment or remote access.

On a configured phone, inspect `systemctl --user status fp6-calls` and D-Bus introspection locally. Service logs do not contain caller names. Verify actual session target, system-bus oFono permission, reserved trusted interface names and app confinement before release. Unknown cellular status blocks outgoing/answer operations. Failure of the assumed DND interface suppresses both ringtone and vibration.

## Manual phone acceptance checklist

Do not mark these checked from laptop mocks. Use synthetic accounts and consented test calls. Full details and expected outcomes are in [PHONE_TEST_PLAN.md](docs/PHONE_TEST_PLAN.md).

- [ ] Connect actual DND, shell/Telepathy, PipeWire, haptic, proximity and history interfaces; enforce the trust boundary.
- [ ] Verify unlocked and locked incoming calls, screen-off wake, redaction, Answer/Decline and return to the locked screen without unlocking.
- [ ] Exercise outgoing, remote end, mute, hold/resume, call waiting, no-answer and failed acknowledgement paths.
- [ ] Test DND off/on during ringing, independent sound/vibration settings, missing policy daemon and exception policy.
- [ ] Test cellular incoming/outgoing/active/held versus VoIP, multiple modems, oFono restart and modem loss. Never place a real emergency call for testing.
- [ ] Check earpiece/speaker/Bluetooth duplex routing, headset hot-unplug, mute, volume and audio restoration on end/crash.
- [ ] Check proximity near/far on earpiece and release on speaker, Bluetooth, hold, end, shell death and broker death.
- [ ] Verify exactly one history record, no duplicate alerts, privacy while locked, clearing/deletion, and intended restart retention.
- [ ] Test malicious/incorrect peers, provider crash, broker/shell/platform crash, stale controls and resource limits.

## Patch series

The supplied repository's `.git` was read-only. Small commits were made in an isolated temporary Git directory and exported as numbered patches under `patches/`; the original Git metadata is unchanged. The workspace already contains the final files. To replay elsewhere, use a clean checkout of task baseline `b3a2947` and `git am /path/to/delivery/patches/*.patch`. Do not apply the series on top of these already-present files. `patches/series` lists the order. Generated patches are ignored to avoid recursively including themselves.

Licensed MIT. Research links and the limits of the implementation are in DESIGN.md; no upstream GPL source was copied into this project.
