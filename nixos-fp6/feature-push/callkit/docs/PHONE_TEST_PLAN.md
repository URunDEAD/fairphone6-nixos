# Phone acceptance plan — not executed

Target: Fairphone 6, aarch64 Mobile NixOS, Lomiri/Qt 5.15/UITK 1.3, oFono, PipeWire/WirePlumber. Test on-device manually after supplying the real adapters. Record exact OS, modem firmware, shell/Telepathy, DND and audio-policy revisions; chosen UI path; session target; headset model; test IDs; pass/fail; observed latency. Use only synthetic caller metadata and controlled test endpoints. No remote host/phone was contacted during this implementation.

## Prerequisites / integration gates

1. Inspect fp6-notifyd and implement the ShouldAlert shim with the actual signature and per-user policy. Verify independent sound/vibration and whether visual wake is permitted. Do not turn errors into alert permission.
2. Choose a single presenter. For Telepathy, complete the CM proof of concept described in DESIGN.md and disable any duplicate surface/ringer/history writer. For separate UI, embed IncomingCall.qml in the trusted shell's call/greeter layer; connect properties/signals through the shell-owned D-Bus controller.
3. Reserve trusted role names through confinement/portal policy. Provider IDs and audio nodes must be resolved independently of their assertions. Test with one confined unauthorized app.
4. Implement Apply with a watchdog and actual board APIs for ringtone, haptics, PipeWire routes, sensor/display ownership and restoration. Acknowledge only capabilities actually applied. Confirm `platform_ready` accurately reflects availability. Test late/stale requests and restart epochs.
5. Implement exactly-once native history integration, or rely on the selected Telepathy observer. Decide retention/consent, opt-in persistence and deletion. Monotonic broker timestamps require epoch conversion for wall-clock UI; do not store them as Unix times.
6. Validate user-service startup/order and oFono system-bus read permission. Keep --demo disabled. Confirm no phone-wide audio-default changes or global microphone mute are used.

## Functional matrix

| ID | Procedure | Required result |
|---|---|---|
| UI-1 | Incoming while unlocked; Answer then remote end | One surface; alert stops; provider confirms answer before Active; returns to previous app |
| UI-2 | Lock, blank screen, report incoming; Decline | Trusted surface visible above greeter; no caller identity leak; phone stays locked |
| UI-3 | Answer while locked, try navigation/notifications | Call controls work; no general app/content access without unlock |
| UI-4 | Rotate, change scaling, enable accessibility, keyboard navigation | Controls reachable, labels unclipped, focus and accessible actions meaningful |
| ST-1 | Incoming → Answer → Hold → Resume → End | One focus owner; pending state until provider acknowledgement; terminal state and one history record |
| ST-2 | Outgoing, connect, remote end; repeat with no remote answer | Dialing is not inbound ringing; active only after connected; timeout after 60 seconds |
| ST-3 | Provider refuses answer/hold/mute/route; delays beyond deadline | Failed request preserves previous state; uncertain command ends after 5 seconds; no silent wrong route |
| ST-4 | Let incoming ring unanswered; remote end twice | Ends at 45 seconds; one no-answer/remote history entry, no lingering alerts |
| DND-1 | Allowed call; enable DND while ringing | Sound/vibration stop within policy-refresh + dispatch interval; visual follows chosen policy |
| DND-2 | Sound off/vibrate on, sound on/vibrate off, both off | Independent channels follow ShouldAlert; no duplicate app-owned ringtone |
| DND-3 | Kill/block/malform DND shim | Silent fallback within bounded refresh; no crash or permanent wait |
| DND-4 | Untrusted app claims favourite/emergency/name | No bypass; real exceptions only through trusted policy resolution |
| CF-1 | Cellular already active/held/dialing; VoIP incoming/outgoing | Incoming waits silently; outgoing and Answer denied; no cellular mutation |
| CF-2 | Active VoIP then cellular incoming/outgoing | VoIP preempted, End sent, media focus revoked by platform; cellular untouched |
| CF-3 | Two VoIP apps; second incoming; hold first and answer second | No two Active calls; pending Answer reserves focus; no auto-resume on call end |
| CF-4 | oFono restart, missing manager, SIM/modem topology change | Unknown blocks activation; active VoIP released; resubscribe/snapshot recovers |
| CF-5 | Simulated emergency condition using test stack | All VoIP ended/denied; never place a real emergency call for this test |
| AU-1 | Earpiece → speaker → Bluetooth duplex → earpiece | Playback and capture use intended ports; no other app/global-default disruption |
| AU-2 | Headset disconnect/reconnect, unavailable route | Clear failure/fallback under policy; no silent caller audio leak to speaker |
| AU-3 | Mute, hold, resume, end, broker/provider crash | Capture and playback conform; previous audio policy restored; no lingering mic |
| PX-1 | Active earpiece near/far repeatedly | Screen blanks only near; unblanks promptly far; no accidental touches |
| PX-2 | Near then speaker/Bluetooth/hold/end/shell or broker death | Display restored; no stale inhibit/blanking beyond lease |
| HS-1 | Answered/missed/rejected/preempted call; clear history | Exactly one entry with correct reason; no contact/name persisted by broker |
| HS-2 | Broker restart and native history adapter restart | Broker history gone by design; native persistence matches consent; no duplicate epoch IDs |

## Fault and security tests

- Use two provider connections. Attempt foreign ReportEnded, Commands, Complete, Snapshot, History and SetProximity. All unauthorized accesses must fail; Calls must reveal only own calls. A self-asserted app ID must not change ownership.
- Kill a provider while ringing/active/held and while a command is pending. It must terminate within the owner-maintenance interval, release effects, and create one record.
- Kill the broker with SIGKILL while ringing and with proximity near. The *real platform adapter* must release effects within 2500 ms without further broker messages. SIGTERM should send the default state immediately. These watchdog guarantees are adapter acceptance criteria, not supplied mock behavior.
- Kill/restart the shell/platform adapter and test name squatting under confined app policy. An unrestricted same-UID process is explicitly outside the current prototype's security boundary.
- Deliver a late Apply after timeout, restart the broker (IDs restart), then try old commands. Adapters/providers must distinguish owner epochs and prevent resurrection of old effects.
- Burst eight live calls, two per provider, and 128 rapid terminal calls. Confirm bounded broker state, understandable errors, cleanup and recovery. Measure provider reconnect spam and add portal-level registration/rate limits before untrusted deployment.
- Test a suspended/unresponsive provider, bus restart, malformed JSON, unavailable Bluetooth, route failures and DND timeout while a cellular call arrives. Check that controls and cellular observer stay responsive and nothing forces cellular hangup.

## Release evidence

Collect synthetic screenshots, test-case results, elapsed times, route/profile observations and privacy-reviewed logs. Do not collect real caller identifiers, audio recordings or credentials. A laptop PNG demonstrates QML rendering only; a mock Apply response demonstrates IPC only. Release requires observed physical ringtone/vibration, lock-screen behavior, hardware routing, proximity and native history on the phone, plus confinement checks. Until then this is an experimental broker/UI prototype.
