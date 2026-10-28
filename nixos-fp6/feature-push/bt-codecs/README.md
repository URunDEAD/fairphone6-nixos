# Fairphone Bluetooth audio

An opt-in NixOS module, Rust session service, and incremental patches for the
existing Lomiri Bluetooth panel and audio-routing indicator. Runtime operations
use libpulse against **pipewire-pulse** and zbus against D-Bus; no subprocesses.

## Integration

Import `module.nix` alongside the existing Bluetooth settings module and set:

```nix
hardware.fairphone6.bluetoothAudio = {
  enable = true;
  preferredCodecs = [ "ldac" "aptx_hd" "aac" "aptx" "sbc_xq" "sbc" ];
  user = "user";            # Lomiri login user (ConditionUser of the service)
  hardwareVolume = null;    # null: keep the system setting (network-fp6.nix: on)
  autoSwitchToHfp = null;   # null: keep WirePlumber's default
};
```

The module defaults off. It does not power or expose the Bluetooth adapter.
All codec plugins provided by the pinned PipeWire build remain enabled; the
preference list is not an allowlist. WirePlumber applies that order when choosing
a profile for a new device, after saved profiles and call policy. The helper's
explicit “prefer quality” action uses the same order. “Prefer stable” selects
SBC when advertised. This is a concrete codec choice, not a measurement of RF
conditions or a promise to eliminate dropouts. WirePlumber stores selected
profiles; the helper has no independent settings database.

`autoSwitchToHfp` enables WirePlumber's microphone-triggered switching and
restoration. Calls must route capture to the Bluetooth microphone. No modem
listener or unsolicited profile switching runs inside the helper. Both
`hardwareVolume` and `autoSwitchToHfp` default to `null`, which writes nothing:
enabling the module does not change existing music or call audio behaviour.
Codec quirks remain under PipeWire's control. The codec-order WirePlumber
script is an `optional` component, so a script problem cannot stop WirePlumber.

The Settings and indicator patches live with the features they change
(integrated 2026-10-28):

- `../bluetooth-settings/lss-bluetooth-audio-quality.patch`: the "Audio
  quality" section on the device page plus its QtDBus bridge. The section is
  hidden while `com.fairphone6.BluetoothAudio1` is not on the session bus, so
  the panel is unchanged when this module is off. Only writes show "Updating".
- `../bluetooth-settings/lss-bluetooth-pairing-lifecycle.patch`: pairing
  prompt lifecycle and input validation (see `REVIEW.md`).
- `../audio-routing/indicator-sound-restore-codec.patch`: restores the exact
  pre-call A2DP profile after the indicator's own call routing.

## D-Bus contract

Session bus name/interface: `com.fairphone6.BluetoothAudio1`

Object: `/com/fairphone6/BluetoothAudio1`

| Method | Signature | Behavior |
| --- | --- | --- |
| ListDevices | `() → s` | JSON schema 1, connected Bluetooth audio devices only; read-only. |
| SetProfile | `(s device, s profile) → ()` | Select one currently advertised, available profile. |
| SetPreference | `(s device, s preference) → ()` | `quality` follows configured order; `stable` selects SBC. Refused while HFP is active. |

JSON contains `schema`, `devices`, and for each device: stable Pulse card `id`,
BlueZ `address`, `name`, active profile ID, current `codec`, optional `bitrate`
in bits/s, `bitrate_source`, and `profiles`. Each profile contains `id`, `label`,
`kind` (A2DP/HFP/LE Audio), `codec`, and `available`. The available codec set is
obtained from available profile entries, never from installed libraries alone.
Unknown codec/bitrate is explicit. No PCM sample-rate multiplication is presented
as Bluetooth bitrate. The 64 kbit/s HFP value is labelled **nominal SCO transport
rate**, not measured audio payload throughput. A2DP variable bitrate is normally
not exported by this API and is shown as “Not reported”.

PipeWire 1.6.7 assigns the base `a2dp-sink` name to the highest-priority codec,
which need not be SBC. The helper resolves the pinned codec description suffix
and uses node `api.bluez5.codec` for the current negotiated codec. Unrecognized
base descriptions remain unknown. Direct profile selection still works.

A session connects only to `$XDG_RUNTIME_DIR/pulse/native`, disables autospawn,
and checks that the server identifies as PipeWire. BlueZ ObjectManager reads use D-Bus `NoAutoStart` (they never activate
bluetoothd) and filter stale cards against actual `Device1.Connected`. PipeWire operations have
a five-second deadline; BlueZ method calls have a three-second deadline. A failed
server operation returns a D-Bus error, with no optimistic local state update.
The Settings bridge is asynchronous and refreshes every two seconds while the
page is visible and Settings is active. A profile-change acknowledgement means
accepted, not that audio transport negotiation is already complete; subsequent
status is authoritative. This API is for applications in the same user session.

## Reproducible checks

The development expressions default to this workspace's pinned FP6 nixpkgs.
Pass `pkgs` explicitly when using another checkout. These commands never access
the phone:

```sh
nix-shell shell.nix --run 'cargo test --locked'
nix-shell shell.nix --run 'cargo clippy --all-targets -- -D warnings'
nix-build default.nix --no-out-link
nix-instantiate --eval --strict --expr 'import ./tests/module.nix {}'
nix-shell -p lua5_4 --run 'lua tests/policy.lua'
nix-build nix/preview.nix --no-out-link
scripts/preview.sh /nix/store/…-fp6-bt-preview
scripts/qml-test.sh /nix/store/…-fp6-bt-preview
nix-build nix/indicator.nix --no-out-link
```

Rust tests start their own private D-Bus daemon and mock the audio backend.
QML tests and screenshots use the real Lomiri UI Toolkit 1.3, software OpenGL,
an offscreen Qt platform, and Xvfb. Pairing tests load dialogs staged from the
built patched Settings panel. Fixtures use only Test Headphones/Test Speaker
and `00:00:00:00:00:01`. See `FINAL_REPORT.md` for build evidence and phone checks.
