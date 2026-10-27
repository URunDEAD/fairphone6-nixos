# FP6 offline voice dictation

A Rust user-session service for Fairphone 6 / aarch64 Mobile NixOS, Lomiri and Maliit. It captures from PipeWire while requested, runs multilingual whisper.cpp through whisper-rs entirely offline, and returns text over D-Bus. Includes `fp6-dictate --seconds 5`, a Lomiri Settings page, explicit verified model downloads, Nix packaging and integration sketches.

**Default:** multilingual `base-q5_1` (about 57 MiB on disk), English or Romanian, eight CPU threads. Tiny and small quantized models are also selectable. No model is bundled. Romanian accuracy remains experimental, with substantial errors in the synthetic evaluation. Phone throughput and thermals still require hardware validation; x86 results are in [engine evaluation](docs/ENGINES.md). Recognition returns a final result, without streaming partials.

## Build and test on the laptop

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell
cargo test
cargo clippy --all-targets -- -D warnings -A clippy::chunks_exact_to_as_chunks
cargo build --release
nix-instantiate --eval --strict tests/module.nix
nix-build -E 'with import <nixpkgs> {}; callPackage ./package.nix {}'
```

An explicit downloader integrity check is available as `nix-shell --run "python3 scripts/check-download.py"` (fetches one model). After benchmarking, `scripts/check-offline-session.py` verifies the complete D-Bus/worker path using synthetic PCM.

The Clippy command allows only the new `chunks_exact_to_as_chunks` syntax-style suggestion; PCM bounds are explicitly validated and tested.

The shell provides stable Rust, CMake, libclang for whisper-rs bindings, D-Bus, Qt 5.15 and Lomiri UI Toolkit 1.3. Tests use a private session bus and synthetic zero PCM, never the host microphone. For plain Cargo, install those native build dependencies and set `LIBCLANG_PATH`. `Cargo.lock` pins Rust dependencies. On-device Rust binaries contain no new unsafe Rust code; whisper-rs wraps upstream native C/C++.

## Enable on NixOS

```nix
{
  imports = [ ./path/to/dictation/module.nix ];
  hardware.fairphone6.dictation.enable = true;
}
```

The module installs the CLI and Settings desktop entry (`X-Lomiri-Touch=true`), enables PipeWire and starts a `systemd --user` D-Bus service with the graphical session. Models and preferences live in user XDG directories. No root daemon or system-bus policy is needed. For a session without `graphical-session.target`, start it explicitly:

```sh
systemctl --user start fp6-dictation.service
fp6-dictate models
fp6-dictate download base-q5_1
fp6-dictation-settings
fp6-dictate --seconds 5
```

The download command is the only network action. It checks catalog size and SHA-256 before replacing a model. Daemon network sockets are restricted to AF_UNIX by systemd. Audio is kept in bounded memory and discarded; transcript text is returned only to the requesting client. Ctrl-C cancels the CLI session. The service accepts one request at a time and discards captures at 30 seconds. CLI durations are 1–29 seconds, leaving room before that limit. Missing models fail before recording starts.

For development without the NixOS module, start `target/release/fp6-dictationd` in the same session bus, then use `target/release/fp6-dictate`. Do not run a second daemon alongside the service. `FP6_CAPTURE_PROGRAM` is a trusted-environment test seam; leave it unset for normal PipeWire recording.

## Preferences

The Settings page selects English/Romanian, tiny/base/small and punctuation. Download another model explicitly before selecting it:

```sh
fp6-dictate download tiny-q5_1
```

Preferences are read at the next Start. They can also be edited while the Settings app is closed:

```ini
# ~/.config/fp6-dictation/settings.ini
[General]
language=ro
model=base-q5_1
punctuation=true
```

Models live at `~/.local/share/fp6-dictation/models` (or `$XDG_DATA_HOME/fp6-dictation/models`). Settings use `$XDG_CONFIG_HOME` when set. Small is experimental and can exceed the default 500 MiB service memory limit; choose tiny/base for the phone. Punctuation off strips common sentence punctuation, preserving Romanian characters and apostrophes.

## Keyboard and microphone indicator

The [D-Bus XML](docs/com.fairphone6.Dictation1.xml) defines `Start`, `Stop`, `Cancel`, `State`, `MicActive` and `StatusChanged`. See [DESIGN.md](DESIGN.md) for ownership and timing rules. Stop may take up to 90 seconds; clients must set a 100-second method timeout.

[Integration instructions](integration/README.md) and the [Maliit patch sketch](integration/maliit-keyboard.patch) cover hold/toggle behavior, early release, cancellation on context change and committing via the input method host. [DictationPrivacyIndicator.qml](integration/DictationPrivacyIndicator.qml) consumes the shell's activity adapter. The daemon tags its real PipeWire capture stream and exposes a state-only privacy signal. **The module does not itself patch Maliit or Lomiri:** the shell hook must be wired into the target builds for the phone's global microphone indicator and keyboard key to appear.

## Offscreen previews

```sh
nix-shell --run scripts/preview.sh
```

This renders the real UITK page with `QT_QPA_PLATFORM=offscreen`, Mesa llvmpipe and Xvfb as the GLX provider. It does not show an on-screen window. UITK's native shapes require an OpenGL context; `QT_QUICK_BACKEND=software` crashes inside the toolkit and is deliberately unset. Temporary settings/cache stay under `/tmp`. Saved previews:

![English preferences](previews/settings-en.png)
![Romanian selection](previews/settings-ro.png)

The Romanian preview demonstrates selected recognition language, not translated interface strings. UI text uses `i18n.tr` where appropriate; a full Romanian UI translation is not supplied.

## Reproduce synthetic benchmarks

```sh
# AVX2 tuning is for this laptop's benchmark binary only.
nix-shell --run 'cargo clean --release -p whisper-rs-sys && GGML_NATIVE=OFF GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON cargo build --release'
export FP6_BENCH_BUILD=avx2-x86
# Isolate evaluation models from normal user data:
export XDG_DATA_HOME="$PWD/benchmarks/work/data"
for model in tiny-q5_1 base-q5_1 small-q5_1; do
  target/release/fp6-dictate download "$model"
done
nix-shell --run 'python3 scripts/benchmark.py'
# Optional English comparison with Vosk and 2026 Moonshine:
nix-shell benchmarks/shell.nix --run 'python3 scripts/prepare-alternatives.py'
nix-shell benchmarks/shell.nix --run 'python3 scripts/benchmark_alternatives.py'
# Additional local neural TTS corpus (optional Piper evaluation dependencies):
nix-shell benchmarks/shell.nix --arg withPiper true --run 'python3 scripts/prepare-piper.py'
nix-shell benchmarks/shell.nix --arg withPiper true --run 'FP6_TTS=piper FP6_BENCH_RESULTS=results-piper.json python3 scripts/benchmark.py'
nix-shell benchmarks/shell.nix --arg withPiper true --run 'FP6_ALT_RESULTS=alternatives-piper.json python3 scripts/benchmark_alternatives.py'
nix-shell --run 'FP6_BENCH_RESULTS=results-piper.json FP6_TEST_LANGUAGE=ro python3 scripts/check-offline-session.py'
```

Changing GGML build flags requires cleaning whisper-rs-sys, since upstream does not track every environment flag for Cargo rebuilds. The Nix package keeps `GGML_NATIVE=OFF` for reproducibility and portability; its aarch64 baseline has NEON, while the generic x86 build lacks AVX acceleration. Do not deploy the AVX2 benchmark binary to another CPU. Initial generic-x86 results are retained in `benchmarks/results-portable.json`.

The four texts in [corpus.json](benchmarks/corpus.json) are synthesized locally with espeak-ng, then resampled to mono PCM16 16 kHz. No human recordings are used. Raw WAVs and downloaded weights are ignored by Git. Piper uses the same texts with the English Lessac and Romanian Mihai medium voices. It replaces the generated WAVs in `benchmarks/work`; use the matching `FP6_BENCH_RESULTS` file when running the D-Bus smoke check.

Measurements include process/model startup, wall time, peak RSS and normalized word error rate; they are a small engineering smoke test, not a representative speech benchmark. Alternative English-only models are not scored as Romanian models.

To transcribe an existing **synthetic test** WAV without microphone capture:

```sh
fp6-dictate transcribe test.wav --language ro --model base-q5_1
```

## Manual checklist for the real phone

- [ ] Build/install on aarch64; verify the user service reaches idle and Settings launches with Suru/UITK on the touch display.
- [ ] In airplane mode with a downloaded model, dictate the synthetic corpus played from a local speaker in English and Romanian. Verify diacritics and punctuation toggle.
- [ ] Measure peak **cgroup** memory (daemon + worker + recorder), model load time, 5/15/29-second utterance wall time, RTF and temperature over ten consecutive runs on eight ARM cores. Target under ~500 MB and RTF ≤1; record any failure rather than extrapolating the laptop result.
- [ ] Verify the microphone indicator lights when PipeWire capture begins, stays visible outside the keyboard, and clears after Stop, Cancel, owner crash, service kill and the 30-second timeout. Keep it lit if another app still records.
- [ ] Test hold/release, release before Start reply, toggle accessibility mode, Cancel during inference, Busy, absent/corrupt models and no available microphone.
- [ ] Test built-in/headset/Bluetooth routing, changing the default source, suspend/resume, and locking during capture.
- [ ] Change focus/app/field while decoding: no text may reach a different field. Disable dictation for password, PIN and hidden-text contexts. Confirm exactly one commit and no clipboard changes.
- [ ] Confirm daemon restart clears all activity, capture descendants exit with the service, journal contains no audio/text, no recording files appear, and no Internet socket is opened by capture/inference.
- [ ] Verify selected settings persist across app/service restart and are applied only on the next capture; test missing model recovery after download.

See [FINAL_REPORT.md](FINAL_REPORT.md) for completed checks, measured results and remaining deployment work.
