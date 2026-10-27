# Final report — FP6 Live Captions

Completed 2026-10-28 (Europe/Bucharest).

Implemented a self-contained Rust/Lomiri live-caption prototype, with verified offline Tiny/Base Q5 models, English/Romanian streaming, bounded rolling text, explicit transcript saving, and NixOS packaging. The laptop implementation runs and has synthetic validation. Phone deployment still requires coordinator integration and hardware validation; the portable x86 latency target was not met.

## Delivered

- `src/streaming.rs`: 20 ms energy gating, 1.5 s windows, 400 ms overlap, silence flush and Unicode overlap merging; bounded 80-word text.
- `src/engine.rs`, `src/models.rs`: persistent whisper-rs CPU decoder adapted from dictation, verified on-demand downloads, catalog size/SHA checks before capture, Tiny/Base Q5, English/Romanian, configurable threads. A 256-position experimental audio context limits padding work.
- D-Bus service and CLI: default sink monitor or explicit microphone, owner lifetime, bounded latest-window queue, stoppable worker, source/decoder timeouts, silence shutdown, call gating and CallAdded cancellation. Unknown call state blocks by default. No model/audio/transcript logging or telemetry.
- Qt 5.15 / Lomiri UITK 1.3 app: normal topmost window hint, drag, collapse, opacity, compact captions, expandable settings, status indication, call opt-in and explicit Save with private permissions.
- `package.nix`, `module.nix`: `hardware.fairphone6.captions.enable`, user service, AF_UNIX-only service networking, CPU/memory/core-dump limits, touch desktop entry.
- Tests, local espeak-ng corpus generation, timing benchmark, four actual offscreen previews, README/manual phone checklist, DESIGN and coordinator privacy/tile/overlay contract.

## Verification

- `cargo test`: **11 tests passed** (8 unit tests, private-bus session test, private-bus mock-oFono test, English/Romanian local TTS segmentation test). Child test-process output repeats two parent results; it does not represent additional tests.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed without command-line lint allowances.
- `scripts/session-smoke.py`: real worker + verified Tiny model + synthetic fake recorder on a private D-Bus; rolling text, monitor/microphone arguments, Stop, controller kill, 60 seconds of accelerated synthetic silence, capture EOF and capture child cleanup passed. Actual PipeWire access is redirected to a nonexistent test endpoint as an additional safeguard.
- `scripts/preview.sh`: English, Romanian, collapsed and settings PNGs rendered with `QT_QPA_PLATFORM=offscreen`. Visual inspection passed; the missing theme tick-icon dependency found in an early preview was removed. Only UITK slow-frame diagnostics remain in the shared laptop environment.
- Nix module enable/disable and privacy/resource assertions: passed.
- aarch64 derivation evaluation: passed. This is **not** an ARM build or device test.
- Final x86 Nix package build: **passed**, including all 11 sandbox tests and installation. Output: `/nix/store/2al7fqi1kld2wg7ls20c8yw2bax4lg9n-fp6-captions-0.1.0`. The installed release binary also passed the synthetic worker/lifecycle smoke test and rendered all four offscreen previews. The initial `/etc/dbus-1/session.conf` dependency found by sandbox testing was fixed with `tests/bus.conf`; no host bus configuration is needed.
- Final patch replay/content audit: **passed**. Six numbered patches apply to a clean checkout at the original task commit; the resulting Git tree and every tracked file match the delivered files. The task brief is unchanged and no cache, model or build artifact is tracked.

## Measured latency

The **installed Nix release** (portable x86, GGML_NATIVE=OFF, no AVX2, Tiny Q5, two threads, Intel Core Ultra 7 155U) produced four windows per local TTS clip:

| Language | Model load | Mean decode/window | First window ready, simulated |
|---|---:|---:|---:|
| English | 0.13 s | 1.08 s | 2.70 s |
| Romanian | 0.09 s | 1.87 s | 3.08 s |

Window availability starts at 1.50 s. These figures exclude PipeWire/display latency and model load from first-window ready time. They come from synthetic speech on a shared laptop, not from the phone or a battery test. The live service drops stale pending windows instead of allowing latency/memory to grow. Both languages returned words; recognition accuracy was not scored. The experimental short context, simple energy VAD and heuristic text overlap need accuracy evaluation.

Total decoder/audio time ratios were 0.90 for English and 1.49 for Romanian; short-window boundaries and synthetic speech limit what these numbers say about real caption quality. The earlier development build measured 4.00/4.25 s first-window readiness; those results remain in `latency-debug.json` and `environment-debug.json`.

The **1–2 second target is not established** by these results. The ARM build has different CPU vector support; do not extrapolate either latency or battery life to FP6. Raw timing/count data and environment notes are in `benchmarks/`.

## Production boundaries

1. Strict call-audio exclusion needs an atomic coordinator/PipeWire routing barrier. The app denies active/unknown oFono call state, subscribes to CallAdded, and polls every 250 ms, but polling/signals alone cannot exclude the instant of a routing transition. A stalled status query can take up to two seconds. Non-oFono VoIP also needs coordinator policy. Explicit per-session call opt-in is off at app launch.
2. Lomiri must wire the global privacy activity aggregator, quick-settings tile, lock/suspend behavior and privileged overlay surface. The normal window hint and in-window indicator are implemented; compositor behavior and global integration are not claimed as shipped shell patches.
3. No physical phone, real microphone/sink or remote host was accessed. aarch64 execution, sink rerouting, call transitions, battery/thermals, memory under the actual service limits and touch/compositor behavior remain on the README checklist.
4. Model downloader integrity logic is tested with synthetic tampering; the real Tiny model was reverified locally. This task did not perform a model network download. Base is selectable/catalogued but not benchmarked in this run. Auto language detection and int8 are not included; the requested quantized option is Q5.

## Read-only reference and repository hygiene

`../dictation` was read only. Its implementation files are working-tree artifacts rather than files committed in its baseline, so the actual reused source hashes are recorded here:

- `src/lib.rs`: `2bde190bbcb2bc804990dbc549938fb68d282a6c7a59a43ee958be890051d6e6`
- `src/bin/fp6-dictate.rs`: `01b37fa58fcb137fd5a65c0ae96ed533022e5ad5eb73a117377dc49e0ee3343f`
- `models.json`: `206769a633534615f6a4c5a85c4a6726ff93352888d3f392301258193e7cb1ec`

MIT attribution is retained in LICENSE. No path dependency points to dictation. Its existing model was copied to `/tmp/fp6-captions-data` for synthetic evaluation. Cargo downloads/build output stay in `/tmp/fp6-captions-cargo` and `/tmp/fp6-captions-target`; Nix outputs stay in the store. Temporary WAVs/PCM are removed by the scripts. No model, Cargo cache or build output is part of the repository or patch series.

The original `.git` was protected read-only, so small commits were made with temporary metadata in `/tmp/fp6-captions-task.git` and exported as numbered mail patches under `patches/`. The original task brief is unchanged. Replay using `git am patches/*.patch` in a writable checkout at `c18ad97`.

1. `655a242` — reusable streaming segmentation and verified offline engine.
2. `21d102b` — caption sessions, D-Bus, Lomiri controls and NixOS packaging.
3. `e75d7af` — call cancellation, private saves, short-window decoding and expanded tests.
4. `cd2b1ef` — isolated D-Bus test configuration (fixes the missing host `/etc/dbus-1/session.conf` found by the Nix sandbox).
5. `64b3a31` — privacy controls display the actual active source/call consent, including CLI-owned sessions.
6. Final delivery commit — previews, benchmark evidence, documentation and this report (hash appears in its patch header).
