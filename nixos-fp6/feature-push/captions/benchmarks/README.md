# Synthetic streaming measurements

`corpus.json` contains English/Romanian text spoken locally by espeak-ng. No human audio is used. `scripts/benchmark.py` creates temporary WAVs and removes them; it asserts that both clips produce several bounded windows and nonempty decoder output. `segmentation.json` exercises the pipeline without a model. Timing files contain counts/timings, not recognized text.

`latency.json` and `environment.json` describe the final installed Nix release. The `*-debug.json` pair retains the initial portable development-build measurements. Release Tiny Q5 / two threads on this shared x86 laptop produced first-window ready estimates of 2.70 s (English) and 3.08 s (Romanian). These estimates include window accumulation and decoding, but exclude model loading, real audio routing and display. They are above the 1–2 s target. They are not phone or accuracy measurements.

The benchmark simulates serial processing of all windows; the live service retains only the latest pending window and drops stale work. Continuous speech, noise and thermals need separate phone validation. See FINAL_REPORT.md and the README checklist.

To reproduce with an installed model and binary:

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell
export CAPTIONS_BIN=/path/to/fp6-captions
# If using a private model directory, set XDG_DATA_HOME accordingly.
scripts/benchmark.py
SEGMENTATION_ONLY=1 scripts/benchmark.py
scripts/session-smoke.py
```

The smoke test uses an explicit private D-Bus configuration, fake PCM recorder, and nonexistent PipeWire endpoint. When pointed at a Nix-wrapped binary it selects the corresponding unwrapped executable so the synthetic recorder remains first in PATH. It tests the real worker without touching real audio hardware.
