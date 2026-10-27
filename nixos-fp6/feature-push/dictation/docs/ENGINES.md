# Offline engine evaluation — 2026-10-27

## Availability and selection

The checked nixpkgs was `26.11pre1100280.3ec636362e18` (`NIX_PATH=nixpkgs=channel:nixos-unstable`). Nix expressions were evaluated locally, rather than assuming package names imply speech recognition support.

| Candidate | Nix / language fit | Decision |
| --- | --- | --- |
| whisper.cpp + whisper-rs | nixpkgs `whisper-cpp` 1.9.5; this crate pins whisper-rs 0.16.0 / whisper-rs-sys 0.15.0, which vendors whisper.cpp **1.8.3**. Multilingual tiny/base/small cover English and Romanian. | Implemented. Q5_1 weights offered; base is the measured bilingual compromise selected as default. Benchmarks use the application's Rust implementation, not the separate nixpkgs CLI version. |
| Vosk 0.3.45 | `vosk`, `vosk-api` and `python3Packages.vosk` were absent in this snapshot. Evaluated its upstream x86_64 manylinux library under Nix with the C++ runtime supplied. Small US English 0.15 is available; no Romanian model is listed in the official catalog checked. | Measured as an English streaming baseline. Not a bilingual default; a production Nix derivation and Romanian acoustic model would be additional work. The evaluation wheel is not shipped. |
| sherpa-onnx 1.13.8 / Moonshine v2 | sherpa-onnx is packaged in nixpkgs. Its February 27, 2026 quantized tiny English model is usable offline; the published v2 language list does not include Romanian. `pkgs.moonshine` is an unrelated Moonlight streaming server. | Measured English candidate with the packaged sherpa CLI. Promising for a future per-language backend, but not a replacement for the bilingual default. |

[whisper.cpp upstream](https://github.com/ggml-org/whisper.cpp) describes CPU/ARM support and quantization. The [official converted-model catalog](https://huggingface.co/ggerganov/whisper.cpp) supplies the weights; exact SHA-256 and byte counts are pinned in [models.json](../models.json). Weight size is not total inference memory: activations, caches, runtime and capture must also fit.

[Vosk's official model list](https://alphacephei.com/vosk/models) provides the small English model and separate optional punctuation models. The latter are unnecessary for this prototype and would substantially increase its footprint. No unsupported Romanian results are invented.

[Moonshine v2 deployment documentation](https://k2-fsa.github.io/sherpa/onnx/moonshine/models-v2.html) provides the quantized 2026 model format used here. [The current Moonshine language list](https://moonshine-voice.readthedocs.io/en/latest/) includes English, Arabic, Mandarin, Japanese, Korean, Spanish, Ukrainian and Vietnamese for STT. A catalog omission is a selection constraint for this task, not proof that no community Romanian model can exist.

## Measurement method

Four original, synthetic utterances: two English and two Romanian. espeak-ng at 145 words/minute produces each clip locally; ffmpeg converts it to mono 16 kHz PCM16. Reference text is checked in at [corpus.json](../benchmarks/corpus.json). No model-supplied or human recordings are used. The corpus is deliberately small; robotic TTS pronunciation can differ substantially from human speech, especially Romanian.

Measurements run on an **Intel Core Ultra 7 155U, x86_64**, Linux, using eight Whisper/Moonshine threads. The full Whisper evaluation explicitly enables AVX, AVX2, FMA and F16C with `GGML_NATIVE=OFF` in the laptop benchmark build. Merely setting `GGML_NATIVE=ON` under this Nix compiler wrapper did not produce AVX instructions in the quantization object; those preliminary rows are separately retained in `results-native-flag.json`. An initial portable x86 build had AVX/FMA/F16C disabled and was much slower; those two preliminary English rows are retained in `results-portable.json`. The shipped Nix build remains portable (`GGML_NATIVE=OFF`), with baseline NEON on aarch64. Each utterance starts a fresh process/model. GNU time measures wall seconds and peak process RSS, including model startup; RTF is wall time / audio duration. WER is word-level Levenshtein distance after Unicode case-folding and punctuation tokenization, retaining diacritics. There is one measured pass per clip/model, no confidence interval. The host was shared and busy; each row records load average so scheduling contention remains visible. These are observed laptop results, not ARM extrapolations or guarantees.

Vosk consumes PCM in 4,000-frame chunks and combines finalized segments and final text; its default CPU configuration is used, rather than claiming an eight-thread setting it does not expose. Moonshine uses the packaged CPU CLI with eight threads. Both are evaluated only on supported English clips. Weights and evaluation source checksums are in [alternative-sources.json](../benchmarks/alternative-sources.json).

The application reloads models per request to free memory and make cancellation reliable. A continuously loaded streaming recognizer could have lower post-release latency, but these cold-process measurements reflect this implementation. Peak process RSS is not complete system/cgroup memory, so the real-phone checklist separately measures the daemon, recorder and worker together. Quantized small is selectable for experiments and is not promised to fit the module's 500 MiB cap.

Raw transcripts are preserved to expose recognition errors rather than reporting only speed. A second corpus uses Piper 1.8.0 with `en_US-lessac-medium` and `ro_RO-mihai-medium`, synthesized locally from the same original texts. Voice file URLs and hashes are recorded in [piper-sources.json](../benchmarks/piper-sources.json). Piper and its evaluation dependencies are not part of the phone package.

Both final datasets were run after this task's compilation stopped, although the shared host was not reserved or idle. Earlier runs during compilation are retained with `-contended` suffixes, and the two unsuccessful generic/native-flag optimization baselines remain separate. No timing rows are silently substituted for those runs. WER retains diacritics and counts `ten` versus `10` as an error; missing Romanian accents are also errors even when the words are otherwise recognizable.

## Final measured results

RTF below 1 means faster than real time **on this laptop in that run**. Each row aggregates two clips: total wall time / total audio time, maximum process RSS, and total word edits / reference words. No ARM speed estimate is inferred.


### Piper synthetic speech

| Model | Language | RTF | Peak RSS (MiB) | WER |
| --- | --- | ---: | ---: | ---: |
| tiny-q5_1 | en | 0.280 | 170.6 | 4.2% |
| tiny-q5_1 | ro | 0.211 | 171.1 | 86.4% |
| base-q5_1 | en | 0.453 | 242.4 | 4.2% |
| base-q5_1 | ro | 0.404 | 242.5 | 40.9% |
| small-q5_1 | en | 1.834 | 481.2 | 4.2% |
| small-q5_1 | ro | 1.591 | 481.3 | 59.1% |
| vosk-small-en-us-0.15 | en | 0.666 | 173.0 | 0.0% |
| moonshine-tiny-en-v2-2026-02-27 | en | 0.369 | 153.4 | 0.0% |


### espeak-ng synthetic speech

| Model | Language | RTF | Peak RSS (MiB) | WER |
| --- | --- | ---: | ---: | ---: |
| tiny-q5_1 | en | 2.321 | 171.2 | 20.8% |
| tiny-q5_1 | ro | 2.096 | 171.6 | 113.6% |
| base-q5_1 | en | 0.845 | 242.3 | 16.7% |
| base-q5_1 | ro | 0.424 | 242.7 | 90.9% |
| small-q5_1 | en | 2.703 | 481.5 | 4.2% |
| small-q5_1 | ro | 1.378 | 482.2 | 68.2% |
| vosk-small-en-us-0.15 | en | 0.639 | 181.5 | 16.7% |
| moonshine-tiny-en-v2-2026-02-27 | en | 0.309 | 153.5 | 29.2% |


## Decision and limitations

Select **multilingual base-q5_1** as the default: 57 MiB of downloaded weights and about 243 MiB peak process RSS leave substantial room below the phone's ~500 MB target. On the Piper corpus, it is faster than real time on this x86 laptop and has lower Romanian WER than tiny. Tiny remains available for a smaller footprint. Small consumes approximately 481–482 MiB by itself, leaving too little margin for daemon/recorder/runtime overhead, and was slower than real time; it is not the phone default.

This does **not** establish near-real-time performance on eight ARM cores. The phone has not been accessed, and native x86/AVX2 throughput cannot be substituted for ARM measurements. The portable x86 Nix package is also not the AVX2 benchmark binary. The manual checklist must measure total cgroup memory, latency, temperatures and repeated utterances on the target.

Romanian quality remains a material limitation. Piper improves the base model's results over espeak, but errors remain substantial even in this tiny corpus. Do not market this prototype as validated Romanian dictation. The small model sometimes omits diacritics, which this metric penalizes. Larger models, better TTS diversity, human evaluation with consent on the eventual phone, or a suitable future Romanian streaming model may be needed. Vosk/Moonshine performed well on these two Piper English clips but cannot replace the bilingual backend without a supported Romanian model.

Final raw results: [Whisper/Piper](../benchmarks/results-piper.json), [alternatives/Piper](../benchmarks/alternatives-piper.json), [Whisper/espeak](../benchmarks/results.json), [alternatives/espeak](../benchmarks/alternatives.json). The corpus is an engineering check, not enough data to rank general recognition accuracy.
