# Final report — 2026-10-27

Implemented an offline dictation prototype with a Rust daemon and CLI, Lomiri preferences, Nix packaging, synthetic benchmarks, and Maliit/privacy integration sketches. **The selected default is multilingual `base-q5_1`.** The phone was not accessed. ARM performance and the external keyboard/shell integration remain unvalidated.

## Delivered

- `src/`, `Cargo.toml`, `Cargo.lock`: stable Rust `fp6-dictationd` and `fp6-dictate --seconds 5`; whisper-rs inference, PipeWire capture, caller-owned D-Bus sessions, Stop/Cancel, disconnect/shutdown cleanup, bounded recording and worker timeout. No custom unsafe Rust.
- `models.json`: tiny/base/small Q5_1 catalog with pinned sizes and SHA-256; explicit HTTPS downloads with atomic replacement. No weights bundled.
- `qml/Settings.qml`: English/Romanian recognition language, model and punctuation preferences using Qt-compatible INI. Real Lomiri UI Toolkit 1.3, with saved offscreen previews in `previews/`.
- `package.nix`, `module.nix`: `rustPlatform.buildRustPackage`, `hardware.fairphone6.dictation.enable`, systemd user service, AF_UNIX-only network policy, memory/core-dump limits, and `X-Lomiri-Touch=true` desktop entry.
- `docs/com.fairphone6.Dictation1.xml`, `integration/`: IPC contract, Maliit mic-key/commit patch sketch, context-change protection, and shell microphone-indicator hook. Capture streams carry stable PipeWire metadata; state signals never contain text.
- `tests/`, `scripts/`, `benchmarks/`: private-bus tests, downloader integrity check, real-worker synthetic smoke tests, locally generated espeak/Piper corpora, and raw measurements/transcripts.
- `README.md`, `DESIGN.md`, `docs/ENGINES.md`: setup, architecture, trust boundaries, engine evaluation, reproducibility instructions and real-phone checklist.

## Verification

| Check | Outcome |
| --- | --- |
| `cargo test` | Passed: 5 library tests, 1 daemon ownership test, 1 private-bus lifecycle test. |
| Lifecycle scenarios | Missing model, Busy, wrong owner, stale ID, Stop, Cancel, disconnect, recorder failure, excessive PCM and SIGTERM tested without a microphone. |
| `cargo fmt -- --check` | Passed. |
| Clippy | Passed with `-D warnings -A clippy::chunks_exact_to_as_chunks`; the sole allowance is a syntax-style suggestion, not a correctness lint. |
| Real inference through D-Bus | Synthetic PCM → capture process → real Whisper worker → CLI text matched direct transcription; English and Romanian exercised. No daemon transcript logging observed. |
| Downloader | Verified real download, corrupted replacement rejection, preservation of existing model, and partial-file cleanup. |
| x86_64 `nix-build` | Passed, including all seven tests inside the Nix sandbox. Final output: `/nix/store/x9i03njrf06knz8ijbmvrvpqcsr6aj4n-fp6-dictation-0.1.0`. |
| NixOS module | Enable/disable, bus name, PipeWire, network restriction and memory-limit assertions passed. |
| aarch64 | Final package derivation instantiated successfully; no ARM binary build or execution claimed. |
| QML | Source and installed package rendered with `QT_QPA_PLATFORM=offscreen`, Mesa software OpenGL and Xvfb. Installed launcher tested with development QML/plugin paths removed. |
| Benchmark artifacts | Python scripts parse and JSON results validate. 32 final measured cases across two TTS corpora, with earlier diagnostic runs retained separately. |

Build snapshot: nixpkgs `26.11pre1100280.3ec636362e18`, Rust 1.99.0, Qt 5.15.19, UITK 1.3.5909. Checks exported `NIX_PATH=nixpkgs=channel:nixos-unstable`; after channel refresh failed, the final checks reused that same resolved snapshot with `-I`. Full tool versions are in `benchmarks/environment.json`.

## Measurements and default choice

The laptop is an Intel Core Ultra 7 155U. Whisper/Moonshine use eight inference threads. Timings include process and model startup. The full Whisper comparison explicitly enables AVX2/FMA/F16C for the laptop; generated object code was checked for AVX instructions. The Nix package remains portable and does not inherit the build host's CPU tuning.

For the default **base-q5_1**, the Piper corpus produced:

| Language | Aggregate real-time factor | Peak process RSS | Word error rate |
| --- | ---: | ---: | ---: |
| English | 0.453 | 242.4 MiB | 4.2% |
| Romanian | 0.404 | 242.5 MiB | 40.9% |

These are two synthetic clips per language, not a general accuracy estimate. Romanian base results improved over tiny's 86.4% WER on the same Piper texts. Small used about 481–482 MiB by itself and was slower than real time, leaving insufficient comfortable margin below the phone budget. Base's approximately 57 MiB download and 243 MiB measured RSS make it the selected bilingual compromise; tiny remains selectable.

Vosk small English and the February 2026 Moonshine tiny English model were also measured. Both recognized the two Piper English clips without word errors, but the checked catalogs provide no Romanian model suitable for replacing the bilingual backend. Vosk was evaluated through its upstream wheel under Nix because the checked nixpkgs lacked a Vosk package; the wheel is not shipped.

The host was shared and sometimes heavily contended. Load averages are retained per row. Initial generic-x86 and ineffective native-flag runs, as well as runs during compilation, are preserved separately. The final espeak and Piper comparisons and full transcripts are linked from `docs/ENGINES.md`. WER preserves diacritics and counts number-format differences such as “ten” versus “10”.

## Remaining deployment limits

1. **Near-real-time on eight ARM cores is not established.** Laptop throughput cannot prove that target. Measure latency, aggregate cgroup RSS, thermals and repeated captures using the README checklist on the phone.
2. **Romanian accuracy remains experimental.** Errors are substantial even on the improved synthetic corpus. This is not validated production-quality Romanian dictation.
3. **Maliit and Lomiri integration is a sketch/hook.** This repository does not modify an external keyboard or shell checkout. The phone's key and global microphone indicator require the documented integration and hardware checks.
4. Final results only; streaming partial recognition is not implemented. Silence suppression is an energy gate, not robust speech VAD.
5. D-Bus session ownership is not an application permission broker. Same-user desktop applications remain trusted; confined-app policy is a deployment concern documented in `DESIGN.md`.

No phone, remote machine, real microphone, human recording or online transcription service was used. Downloads were development dependencies, upstream model/source artifacts, and local TTS voice models. All evaluated speech was synthesized locally from the checked-in original texts.

## Commit delivery

The workspace's `.git` is mounted read-only. The requested normal commit failed with `Unable to create .../.git/index.lock: Read-only file system`. Small commits were therefore created in an isolated Git directory, retaining the original task commit as their base. The complete history, including this report and final previews, is delivered in **`task-commits.bundle`**. All finished source files are also present in this workspace; its original Git metadata was not changed.

To obtain a normal committed checkout without overwriting these working files:

```sh
git bundle verify task-commits.bundle
git clone task-commits.bundle ../dictation-completed
```
