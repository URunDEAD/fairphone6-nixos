# FP6 Live Captions

Private, offline English/Romanian captions for Fairphone 6 / aarch64 Mobile NixOS with Lomiri, Qt 5.15 and UITK 1.3. Includes a Rust service, a draggable/collapsible caption window, and a CLI. Device audio is the default source; microphone capture is optional and explicit.

The multilingual tiny/base Q5 models and CPU whisper-rs decoder are adapted from the MIT-licensed `../dictation` reference. This project is self-contained; it does not modify, build, or depend on that directory. No model is bundled. See [DESIGN.md](DESIGN.md) for the streaming/upstream boundary and limitations.

## Build and test

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
scripts/preview.sh
nix-instantiate --eval --strict tests/module.nix
nix-build --no-out-link -E 'with import <nixpkgs> {}; callPackage ./package.nix {}'
```

The shell puts Cargo downloads in `/tmp/fp6-captions-cargo` and build artifacts in `/tmp/fp6-captions-target`. Neither cache nor weights belong in this repository. Plain Cargo requires stable Rust, CMake, libclang (`LIBCLANG_PATH`), Qt development files/qmake, D-Bus, espeak-ng and ffmpeg. `cargo test` generates its English/Romanian test WAVs locally in temporary directories and removes them. It does not open a real audio device or download weights. Package checks use the same tests.

## Install and use

```nix
{
  imports = [ ./captions/module.nix ];
  hardware.fairphone6.captions.enable = true;
}
```

```sh
systemctl --user start fp6-captions.service
fp6-captions models
fp6-captions download tiny-q5_1
fp6-captions-ui
```

Download is an explicit CLI action using HTTPS and catalog size/SHA-256 verification. Models are verified again before capture. Weights live under `$XDG_DATA_HOME/fp6-captions/models` (default `~/.local/share/fp6-captions/models`). Select Base in the UI only after `fp6-captions download base-q5_1`. No automatic network requests occur in the service or UI; the service permits only Unix sockets.

For a development session, run `fp6-captions daemon` before the UI using the binaries in `$CARGO_TARGET_DIR/debug`. The daemon starts idle. The UI keeps its D-Bus connection open as the session owner; closing/crashing it stops capture. The CLI can also own a session:

```sh
fp6-captions start --language ro --model tiny-q5_1 --threads 2 --silence-minutes 3
# Ctrl-C stops. Another terminal can stop it too:
fp6-captions stop
fp6-captions status
```

Start is blocked if oFono cannot establish that all modems have no calls. Ringing, dialing, active and held calls all block capture. On a laptop without oFono, deliberate testing requires the session-only `--allow-calls` flag (or the UI toggle, off at launch). This also permits capture when call state is unknown; do not enable it casually. See the call-routing boundary in DESIGN.md before deploying on a phone.

The window offers language, model, audio source, CPU threads (2/4), silence timeout (1/3/5 minutes), opacity, collapse and drag. CLI bounds are 1–8 threads and 1–30 minutes. Restart capture to apply changes. The normal always-on-top hint and window movement depend on compositor support; Lomiri shell overlay/quick-settings/privacy hooks are specified in [docs/COORDINATOR.md](docs/COORDINATOR.md), not silently installed as shell patches.

Text is held only in a bounded 80-word rolling buffer, expires after silence, and clears on Stop. **Save visible text** explicitly writes just that buffer to `~/.local/share/fp6-captions/saved/captions-<time>.txt` (XDG-aware, mode 0600). There is no continuous transcript history, audio file, clipboard write, telemetry, or transcript logging. `status` intentionally returns current text to its caller; do not redirect it to a log if you want no transcript storage.

## Preview and measurements

![English caption window](previews/captions-en.png)
![Romanian caption window](previews/captions-ro.png)
![Collapsed window](previews/captions-collapsed.png)
![Settings](previews/captions-settings.png)

`scripts/preview.sh` renders the actual QML with `QT_QPA_PLATFORM=offscreen`, Mesa software GL and Xvfb as a GLX provider. Preview mode never connects to the caption service and uses synthetic sample text. UI settings/cache are isolated under `/tmp`. UITK shapes require GL; do not use `QT_QUICK_BACKEND=software`.

```sh
# No model needed for segmentation checks:
SEGMENTATION_ONLY=1 scripts/benchmark.py
# After explicitly installing a tiny model:
scripts/benchmark.py
# Extended private-bus test with the real worker and a fake synthetic recorder:
scripts/session-smoke.py
# Or choose a synthetic WAV/model directly:
fp6-captions benchmark test.wav --language ro --model base-q5_1 --threads 2
```

The script synthesizes the two corpus sentences locally with espeak-ng, resamples with ffmpeg, and removes WAVs afterward. JSON results contain timings/counts, not recognized text. The decoder benchmark reports model load and per-window decode time; serial ready times are simulated and exclude PipeWire/display latency. The live daemon instead drops stale pending windows. The 1–2 second display goal is **not a phone performance guarantee**; see the measured findings in FINAL_REPORT.md.

Tiny Q5 uses about 31 MiB of disk; Base Q5 about 57 MiB. Q5 is the supported quantization option (int8 is not advertised). Continuous decoding costs CPU/battery, and Base usually costs more. Start with Tiny and two threads; the service is capped at two CPU equivalents and 600 MiB. Increasing threads above two needs a deliberate module CPUQuota override to gain throughput. Silence gates inference immediately and stops capture after the chosen timeout. Music/noise can trigger the energy gate and prevent automatic silence shutdown. No power or thermal measurements have been made on the phone.

## Manual phone checklist

- [ ] Build/install on aarch64, launch from Lomiri and verify Suru controls, touch targets, diacritics, drag, opacity, collapse and always-on-top behavior in phone/desktop modes.
- [ ] Download once, switch to airplane mode, and caption local synthetic English/Romanian videos/podcasts/voice-message playback. Compare intelligibility and word-boundary duplication/missing words.
- [ ] Measure time from played speech to displayed text, dropped windows, cgroup CPU/memory, battery drain and temperature for Tiny/Base over 15 minutes. Test 2/4 threads with the configured CPU cap. Do not infer phone results from x86 timings.
- [ ] Verify sink-monitor capture does not select the microphone, follows the default sink with speaker/headset/Bluetooth changes, and fails safely when the sink disappears. Check microphone mode separately.
- [ ] With override off, test ringing/dialing/active/held calls on every SIM, oFono restart/removal, unknown status, and call start during model load/inference. Verify the coordinator's atomic call-route exclusion before production use. Test explicit opt-in separately.
- [ ] Confirm global audio/microphone privacy activity appears while capturing (including collapsed/background window), survives another recorder starting/stopping, and clears on Stop, controller crash and service kill.
- [ ] Confirm quick-settings reflects actual service state, requires deliberate activation and stops on session lock/suspend according to coordinator policy.
- [ ] Silence-stop at 1/3/5 minutes; no decoder CPU on silence; no late captions after stopping; safe behavior on stalled capture, slow model and missing/corrupt model.
- [ ] Confirm Stop/crash/restart leaves no WAV/transcript files; Save alone creates a private file containing only visible text; inspect journal for absence of text/audio. Confirm no daemon Internet sockets.
- [ ] Confirm background app suspension policy does not freeze the controller while leaving capture running; test display off, session lock, suspend/resume, application switch and service restart.

The original `.git` is read-only in this environment. Small commits are preserved as a numbered mail-patch series under `patches/`; see `patches/README.md` for replay instructions.
