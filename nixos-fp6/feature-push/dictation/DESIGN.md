# Offline dictation design

`fp6-dictationd` owns `com.fairphone6.Dictation1` on the **user session bus**, at `/com/fairphone6/Dictation1`. The daemon, command-line client and transcription implementation are stable Rust with `forbid(unsafe_code)`. whisper-rs supplies the audited upstream FFI boundary to native whisper.cpp; its C/C++ implementation is necessarily outside that Rust guarantee.

## Lifecycle and IPC

| Call/property | Contract |
| --- | --- |
| `Start() -> t` | Snapshot preferences, check the selected model exists at its expected size, reserve one caller-owned session and launch capture. Returns an opaque 64-bit generation ID. |
| `Stop(t) -> s` | Only the originating unique bus name may stop that ID. Close capture, transcribe offline, and return UTF-8 text **only in the method reply**. Allow 100 seconds for the call. |
| `Cancel(t)` | Owner-only; discard pending audio/text, terminate capture or worker. Asynchronous cleanup follows. |
| `State: s` | `idle`, `starting`, `recording`, `transcribing`. |
| `MicActive: b` | Conservative capture-process indicator. True during the recording phase. |
| `StatusChanged(s,b)` | Public state-only hook; never contains recognized text or audio. Read properties on connection/reconnection. Properties do not emit `PropertiesChanged`; subscribe to this signal. |

Only one session can exist, including while Stop is pending. ID and owner checks reject stale and cross-client operations. Methods use `&self`, so Cancel remains dispatchable during a pending Stop. The owner is checked on the bus every 250 ms; disconnect cancels the operation. Stop detaches the reply receiver without holding the state lock during inference. No transcript history is retained.

The hard capture deadline is 30 seconds; reaching it discards the recording and returns to idle, without automatically inserting text. PipeWire EOF and process failure also reset to idle. A client must watch status and reset its local button on idle. At most 960,000 raw PCM bytes are accumulated. Stop kills and reaps the recording child before inference begins. The worker reads a bounded stdin, uses eight inference threads, and has a 90-second deadline. Cancellation drops kill-on-drop child handles. systemd's control-group kill handles daemon termination and descendants. Audio never becomes a recording file.

PipeWire's `pw-record` supplies mono, 16 kHz, signed little-endian PCM via stdout. Using the packaged capture utility avoids duplicating PipeWire's negotiation/resampling and removes a second native Rust FFI dependency. Its stderr is discarded, as is the inference worker's stderr; speech is not sent to the journal. Named stream properties identify `FP6-Dictation` / `fp6-dictation`, with media role `Communication`. No default source is overridden: WirePlumber's user-selected microphone is respected.

A separate worker process permits reliable cancellation even during a synchronous native inference call, releases all model allocations between utterances, and reduces idle RSS. Model loading adds latency, measured in the benchmarks. Final results only: repeatedly decoding overlapping Whisper windows on a phone would multiply CPU use and introduce unstable word revisions. A future streaming backend can add a **unicast**, session-tagged partial signal and preedit updates; this version never claims partial recognition.

## Model and language policy

The default is multilingual `base-q5_1`, not the English-only `.en` variant. It supports both requested languages and delivered a better Romanian accuracy/memory compromise than tiny on the local synthetic corpus, at about 243 MiB peak process RSS. `tiny-q5_1` is selectable to reduce memory and compute; `small-q5_1` is an explicit experimental option that may exceed the module's 500 MiB cgroup budget. The QML page warns about that limit. Accuracy on Romanian synthetic speech is reported without implying human-speech accuracy. See [engine evaluation](docs/ENGINES.md) and the checked-in raw benchmark results.

Models are not bundled or fetched by the daemon. `fp6-dictate download ID` downloads only allowlisted HTTPS model files, checks exact length and pinned SHA-256, then atomically renames the partial file. A failed download preserves an existing installation. The model catalog is `models.json`; mutable upstream URLs cannot silently change accepted weights because hashes are pinned. Downloading is a separate explicit CLI action, outside the daemon's network-restricted service. No URL or path can be supplied over D-Bus. The model directory belongs to the user and is trusted; Start's size check is not intended to defend against a malicious same-UID writer replacing model files.

Preferences are Qt-compatible INI at `$XDG_CONFIG_HOME/fp6-dictation/settings.ini`, falling back to `~/.config`. Qt.labs.settings writes strings and a boolean; Rust validates language/model IDs on every Start. Settings changes affect the next request. Punctuation uses Whisper's natural output; disabling it removes common sentence punctuation while preserving apostrophes, hyphens and Romanian diacritics. It is not a separate punctuation restoration model. An energy gate returns empty text for silence; this is not robust VAD, and Whisper can hallucinate on other non-speech sounds.

## Privacy and deployment boundary

No telemetry, remote recognition, automatic model downloads, audio persistence, clipboard injection, or transcript logging. The user service allows AF_UNIX only, so recording and inference cannot open Internet sockets. It disables core dumps, limits aggregate memory to 500 MiB and stops with the graphical session. Cleared allocations are freed, not cryptographically erased; normal memory/swap policy still applies. The target deployment should use encrypted or disabled swap if required.

The session bus is a same-user trust boundary, not an application permission broker. Another application in the same desktop session can ask to start its own recording while idle. A production confined-app policy must mediate bus access or use a trusted keyboard broker; hiding the service name is not authorization. Owner checks protect session control and transcript replies from unrelated clients, but privileged bus monitors and the same Unix account are not treated as adversaries.

The included privacy hook exposes conservative capture activity and stable PipeWire metadata. Actual global microphone indication belongs in Lomiri's shell. [Integration instructions](integration/README.md) describe wiring that hook and observing real PipeWire recording nodes, including crash recovery. Enabling this Nix module alone does not patch an external Maliit or Lomiri source tree.

## Validation scope

Unit tests cover settings, text normalization, PCM limits, silence and model IDs. A private D-Bus integration test uses an executable emitting zero PCM: missing-model handling, Busy, ownership, stale IDs, Stop, Cancel, owner disappearance and recorder failure. No test opens the laptop microphone. The synthetic benchmark runs real offline models on locally synthesized English/Romanian WAV files. QML previews use the actual UITK with `QT_QPA_PLATFORM=offscreen`, Mesa software OpenGL and Xvfb; UITK's Shape renderer cannot use Qt Quick's software scene graph. Phone routing, sustained thermals, ARM throughput and shell integration require the manual checklist.
