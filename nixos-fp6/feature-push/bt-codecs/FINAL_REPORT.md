# Bluetooth audio codecs and pairing — delivery

Implemented an opt-in `hardware.fairphone6.bluetoothAudio` module, Rust/zbus
session daemon, native PipeWire Pulse API backend, Lomiri UI Toolkit 1.3 audio
controls, pairing fixes and an audio-routing codec-restoration patch. Nothing
was deployed or activated. No phone access, git commits, shared feature edits,
pairing bypasses or Bluetooth scans were performed.

## Coordinator integration

Exact import for the delivered workspace:

```nix
imports = [ ./bt-codecs/module.nix ];
hardware.fairphone6.bluetoothAudio.enable = true;
```

When copying this directory to `nixos-fp6/feature-push/bt-codecs`, use
`./bt-codecs/module.nix` in the coordinator's import list. The full trial
already imports/enables `bluetoothSettings` and `audioRouting`; preserve those.
The new feature defaults off. (Integration note: `hardwareVolume` and
`autoSwitchToHfp` were changed to default `null` = keep the system setting,
the patches were moved into bluetooth-settings/ and audio-routing/, and the
user defaults to "user"; see README.md.) See `README.md` for all options and the D-Bus
schema. The test/build convenience expressions use the current private
checkout's pinned nixpkgs; the module/package themselves are relocatable.

## Codec audit of the actual pin

Pin: nixpkgs `0bb7ec54c848`, channel artifact
`nixos-26.11pre1031299.0bb7ec54c848`; PipeWire **1.6.7**, WirePlumber **0.5.15**.
Inspected both nixpkgs build inputs/meson flags and PipeWire's codec build
rules. Verified the plugin files in the realized **aarch64** PipeWire output,
not only on the development host. Evidence is in `evidence/codec-build.json`.

| Requested codec | Installed plugin under `lib/spa-0.2/bluez5` | Override needed? |
| --- | --- | --- |
| SBC / SBC-XQ | `libspa-codec-bluez5-sbc.so` | No; XQ is a mode in the SBC plugin. |
| AAC | `libspa-codec-bluez5-aac.so` | No; pinned package includes `fdk_aac`. |
| aptX / aptX HD | `libspa-codec-bluez5-aptx.so` | No; pinned package includes `libfreeaptx`. |
| LDAC | `libspa-codec-bluez5-ldac.so` | No; `ldacbt` is available on aarch64 and enabled. |
| LC3, LE Audio | `libspa-codec-bluez5-lc3.so` | No; `liblc3` and the LC3 meson option are enabled. |
| mSBC, calls | `libspa-codec-bluez5-hfp-msbc.so` | No; native HFP/HSP backends and SBC dependencies are present. |
| LC3-SWB, calls | `libspa-codec-bluez5-hfp-lc3-swb.so` | No; built with LC3 support. |

Also installed: CVSD, LC3-24kHz/A127, FastStream, Opus/Opus-G and G.722 plugins.
**LC3plus is distinct from LC3** and is explicitly disabled in this nixpkgs
package because its source is not readily obtainable. Enabling LC3plus would
require supplying its source/library and overriding the build; this delivery
does not do so. None of the requested codecs needs a package override.

The module leaves the codec allowlist at PipeWire's all-available default and
preserves hardware quirks. Compiled support does not prove a peripheral,
controller, firmware, BlueZ transport or current telephony backend can
negotiate that codec. LE Audio and LC3-SWB especially require phone tests.
Codec order is implemented in a WirePlumber selection hook, rather than
pretending that `bluez5.codecs` list order controls priority. Saved choices and
call policy retain precedence.

Primary references checked against the pinned source:
[PipeWire 1.6.7 codec build rules](https://gitlab.freedesktop.org/pipewire/pipewire/-/blob/1.6.7/spa/plugins/bluez5/meson.build),
[nixpkgs PipeWire package at the pin](https://github.com/NixOS/nixpkgs/blob/0bb7ec54c848/pkgs/by-name/pi/pipewire/package.nix),
[WirePlumber Bluetooth configuration](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/bluetooth.html).
The online WirePlumber documentation is newer than the pin; local 0.5.15
scripts were used to verify hook names and settings.

## Behavior and limitations

- Status reads use libpulse introspection and BlueZ ObjectManager reads. They
  never choose a profile, power an adapter, initiate pairing, or change volume.
- Codec selection uses the supported libpulse card-profile API. The pinned
  pipewire-pulse implementation translates this to PipeWire `SPA_PARAM_Profile`
  with `save=true`; no shell commands or ad-hoc writes to `bluez5.codec`.
- Only connected BlueZ audio cards are returned. Advertised profiles supply the
  available codec list. Base `a2dp-sink` is resolved carefully: in this pin it
  aliases the highest-priority codec, not necessarily SBC.
- A2DP bitrate generally is not exposed by this API; the UI says “Not reported”.
  HFP's known 64 kbit/s value is explicitly labelled nominal SCO transport rate,
  never a measurement. PCM stream bitrate is not misrepresented as RF bitrate.
- “Prefer stable” explicitly selects SBC; “Prefer quality” uses configured
  order. These are per-device saved profile choices, not RF-quality monitoring.
- HFP automatic switching is capture-triggered WirePlumber behavior. It does
  not listen to modem call state; call audio must actually use the Bluetooth
  microphone. Existing indicator call routing is retained.
- QML polls asynchronously while visible. An accepted profile request is
  followed by a fresh status read; transport renegotiation can finish later.
- Pairing review and additional phone-dependent findings are in `REVIEW.md`.
  The supplied patches add prompt tracking/cancellation, a 60-second prompt
  timeout, passkey/PIN validation and failure feedback. Patch 0003 preserves
  the exact music codec when the indicator restores audio after its own call
  routing. It falls back to existing policy if that profile disappears.

## Validation and build evidence

Passed:

- `cargo test --locked`: 7 tests, including a private D-Bus contract test with a
  mocked audio backend, read-only invariants, capability validation, disconnect
  errors, unknown alias handling, stable/quality resolution, and a mock BlueZ
  ObjectManager verifying the actual D-Bus `NoAutoStart` message flag.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt`.
- Nix helper package build, including its Rust tests inside the Nix sandbox.
- Installed-daemon smoke test: introspection and three failed-server reads;
  the daemon stayed alive and did not create/autospawn an audio server.
- QML runner: 9 passes including setup/cleanup, zero failures. Tests cover
  non-mutating bindings, an explicit profile selection, disconnect refresh,
  passkey validation and legacy PIN validation. Only the toolkit's existing
  deprecated `caller` notice remains.
- Six mock WirePlumber policy cases, including saved/call profile precedence,
  availability filtering, non-Bluetooth exclusion and unknown-codec fallback.
- Module assertions verify disabled-by-default behavior, hardware-volume/HFP
  defaults, configured codec order and no adapter enablement.
- Compiled the patched C++ Bluetooth panel and patched Vala sound indicator.
- Six real UITK offscreen screenshots in `previews/`; these use isolated
  synthetic fixtures, not live device negotiation or a running phone Settings app.
- Full module/system evaluation succeeded. Shared inputs changed during this
  task, so system derivations from separate evaluations can differ.

Final x86_64 outputs:

| Artifact | Built store path |
| --- | --- |
| Rust helper | `/nix/store/f17haidvsbgrr20qwcfmyzq1ybk67a00-fp6-bluetooth-audio-0.1.0` |
| Bluetooth panel | `/nix/store/slr162hb3z3f2rga55szjm9c07c9x5r0-lomiri-system-settings-bluetooth-fp6-1.3.2` |
| Sound indicator | `/nix/store/n13jwbcml8114z86ivf32ka7fgpn2frr-ayatana-indicator-sound-fp6-24.5.3` |
| Preview environment | `/nix/store/ni4av3v0g943pfqaf33kqhjzaqsbra8n-fp6-bt-preview` |

**Full ARM system build blocker:** the shared `fp6-gestures` package failed with
Rust E0560/E0609: `Config.back_sensitivity` does not exist (`src/main.rs:710`).
That source is outside this task's writable scope and was not changed. See
`evidence/arm-system-build-failure.txt`; no successful system closure is claimed.
The standard builder wrapper also initially failed on local SSH config
permissions; the retry used the same configured Nix builder store with
`NIX_SSHOPTS='-F /dev/null -oIdentitiesOnly=yes -oBatchMode=yes'` and the already
established tunnel. No shared SSH configuration was modified.

All three ARM leaf builds succeeded independently of the unrelated gestures
failure. The final helper includes the tested `NoAutoStart` change.

| aarch64 artifact | Built store path |
| --- | --- |
| Rust helper | `/nix/store/mqg5p14m7h6lv0mb0sm2sw5jmvdvys8n-fp6-bluetooth-audio-0.1.0` |
| Bluetooth panel | `/nix/store/7fy726f1s4bg4k8dh4902brrws8hkpip-lomiri-system-settings-bluetooth-fp6-1.3.2` |
| Sound indicator | `/nix/store/j6lvc26a68iy1pkbsm5sad50f8ymlxjz-ayatana-indicator-sound-fp6-24.5.3` |

See `evidence/arm-leaves-build.txt` and `evidence/arm-helper-build.txt`.
`evidence/arm-paths.json` is an earlier **evaluation snapshot**, not by itself
proof that each output was built. Its helper entry predates the final
`NoAutoStart` revision; the table above and build log identify the final binary.


## Phone test checklist for the coordinator

- [ ] Enable the module; confirm the user service owns the session D-Bus name
  and Settings finds it. Check confined Settings access if AppArmor is enabled.
- [ ] Open and refresh device status repeatedly: no codec, profile, volume,
  adapter power, visibility or pairing changes occur from reads.
- [ ] With each supported headset, confirm reported current codec against
  PipeWire/BlueZ; select every advertised A2DP/HFP profile and observe errors
  for a device disconnected during a change.
- [ ] Test SBC, SBC-XQ, AAC, aptX, aptX HD and LDAC on compatible devices;
  separately test LE Audio LC3 and HFP mSBC/LC3-SWB where supported.
- [ ] Choose SBC stability, disconnect/reconnect and restart the session;
  verify WirePlumber restores the choice. Repeat with a quality codec.
- [ ] Enable `autoSwitchToHfp`, place/receive a call routed to Bluetooth,
  verify microphone capture, and verify the original music profile returns.
  Repeat with the indicator's explicit routing and after switching devices.
- [ ] Leave `hardwareVolume=null` (system: on), then try `false`/`true` and verify synchronization,
  mute and volume steps with a compatible headset.
- [ ] Test numeric comparison, leading-zero passkey entry, invalid/empty
  input, alphanumeric legacy PIN, user cancellation and remote cancellation.
- [ ] Leave a prompt unanswered for 60 seconds, background Settings, power
  cycle Bluetooth, restart BlueZ and remove a selected device. No stale
  prompts or indefinite busy indicators should remain.
- [ ] Verify reconnect expectations and UI text; check failure feedback and
  small-screen layout in the real Bluetooth device page.
