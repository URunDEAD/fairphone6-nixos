# Bluetooth pairing and audio review

Reviewed the pinned lomiri-system-settings 1.3.2 source with the existing
`bluetooth-settings/lss-bluetooth-fp6.patch` applied, and
`audio-routing/indicator-sound-device-selection.patch`. Shared files were read
only. Patches in this directory apply **after** those feature patches.

Integrated (2026-10-28) as: patch 0001 =
`bluetooth-settings/lss-bluetooth-audio-quality.patch` (plus: section hidden
without the helper, no busy flicker on polls), 0002 =
`bluetooth-settings/lss-bluetooth-pairing-lifecycle.patch`, 0003 =
`audio-routing/indicator-sound-restore-codec.patch`.

| Finding | User-visible consequence | Disposition |
| --- | --- | --- |
| `PageComponent.qml` does not retain confirmation, PIN-entry, or passkey-entry popup handles. Only display-only dialogs are tracked. | BlueZ Cancel/Release cannot close these prompts; a stale prompt can remain after pairing fails. | Patch 0002 tracks all three request types, clears their handles on answers, and closes them on cancellation/completion. |
| Incoming input/confirmation requests can open overlapping dialogs. | The user can answer the wrong request; the older dialog has no coherent lifecycle. | Patch 0002 refuses the new input request while a prompt is already visible. No automatic confirmation is added. |
| Input prompts have no UI deadline. BlueZ visibility/pairability timeouts are not prompt timeouts. | A prompt can remain indefinitely if the transaction disappears without a UI update. | Patch 0002 adds a 60-second prompt deadline, replies with cancellation, and closes the prompt. Display-only requests call the existing CancelPairing path. |
| `Agent::Cancel` and `Release` retain delayed reply entries. | Later callbacks can refer to abandoned requests, and the map grows. | Patch 0002 clears abandoned entries when BlueZ ends the transaction. |
| `ProvidePasskeyDialog.qml` pre-fills `000000`, accepts unrestricted text, and calls `parseInt` without validation. | A blank/non-numeric/out-of-range passkey can reach the backend. | Patch 0002 starts empty, restricts to 1–6 decimal digits, and disables Pair for invalid input. A leading-zero passkey remains valid. |
| Legacy PIN dialog pre-fills `0000` and permits an empty submission. | It encourages an unverified PIN guess. | Patch 0002 starts empty and requires nonempty input; the existing 16-character limit remains. Alphanumeric legacy PINs remain supported. |
| Pairing completion discards the `success` argument. | A failed attempt looks like a disappearing dialog without explanation. | Patch 0002 adds a short, non-identifying failure/timeout message on the Bluetooth page. |
| The trust checkbox says “Connect automatically when detected”, but only changes BlueZ `Trusted`. | Users expect active discovery/reconnection that the checkbox does not implement. | Patch 0001 corrects the label to “Allow connections from this paired device”; reconnect behavior is unchanged. |
| The audio indicator restores the highest-priority A2DP profile after its own call routing. | A user who selected SBC for stability can unexpectedly return to LDAC. | Patch 0003 records the exact active A2DP profile before the indicator's call selection and restores it if still available, falling back to existing priority policy otherwise. |

## Further fixes requiring phone/integration tests

- `Device::connect()` and `disconnect()` mostly report failures in the journal.
  Add a typed operation-result signal and per-device inline retry message, with
  distinct states for pairing, profile connection and transport readiness. A
  failed call must not set a “connected” UI state. Avoid an automatic retry loop.
- The main page already suppresses repeated taps while pairing/connecting; the
  device page treats Connecting as connected and offers Disconnect. Verify that
  cancellation during Pair, rather than during Connect, consistently invokes
  CancelPairing and clears the temporary pairability timer.
- BlueZ's 180-second visibility/pairability deadline and the backend's temporary
  pairability window are separate from D-Bus operation deadlines. The added
  60-second prompt deadline does not cancel a pairing operation that has never
  requested a prompt. Add an operation watchdog only after testing the current
  BlueZ Agent and device watcher lifecycle; do not blindly retry Pair.
- The existing display-passkey UI removes entered digits from the visible code.
  Consider showing the full six-digit code with a separate progress indicator
  for easier comparison, and test repeated DisplayPasskey updates. The patch
  only accepts progress updates for the same tracked device.
- Adapter power cycling, BlueZ restart, Settings backgrounding and deletion of a
  selected device should dismiss prompts and reset progress. The new tracking
  supports Cancel/Release, but these lifecycle sequences still need device tests.
- Reconnection is currently driven by BlueZ, the peripheral and existing
  WirePlumber policy, not by the trust checkbox. An explicit future reconnect
  preference should specify permitted profiles and a bounded backoff, without
  making the phone discoverable or initiating pairing.
- WirePlumber automatically switches when an application actually captures from
  the Bluetooth microphone. This does not itself observe modem call state.
  Verify that Lomiri telephony routes its call capture stream there. The
  indicator's existing explicit call routing and WirePlumber must not compete.
- Patch 0003's memory belongs to the running indicator. If the indicator restarts
  during a call or another component already switched to HFP, it falls back to
  the existing policy; WirePlumber owns its own restore state. A unified routing
  owner would be a larger follow-up.

The review introduces no pairing bypass, new service authorization, device
scanning, or radio activation. Existing numeric-confirmation and authorization
prompts still require an explicit user response.
