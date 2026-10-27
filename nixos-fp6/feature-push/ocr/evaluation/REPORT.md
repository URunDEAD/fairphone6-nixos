# OCR engine evaluation

Measured on 2026-10-27 on this x86_64 laptop (Intel Core Ultra 7 155U).
Nixpkgs: `26.11pre1100280.3ec636362e18`.

**Decision: ship Tesseract 5.5.3 with English and Romanian data.** Its Romanian
accuracy and lower measured CPU cost / memory make it the better fit for this
app. `ocrs` 0.13.1 / RTen 0.26.0 was actually built and run with the upstream
default models, but those models performed poorly on Romanian diacritics.

## Results

| Synthetic case | Engine / language | CER | WER | Median wall ms | Median CPU ms | Peak RSS MiB |
|---|---|---:|---:|---:|---:|---:|
| english | Tesseract / eng | 0.00% | 0.00% | 5102.0 | 1310 | 89.1 |
| english | ocrs / default | 0.95% | 5.56% | 2689.7 | 2290 | 161.1 |
| romanian | Tesseract / ron | 0.96% | 5.26% | 3051.3 | 1130 | 55.4 |
| romanian | ocrs / default | 20.19% | 84.21% | 4382.7 | 2080 | 161.0 |
| mixed | Tesseract / eng+ron | 12.20% | 53.33% | 5362.8 | 1690 | 103.5 |
| mixed | ocrs / default | 18.29% | 66.67% | 2516.7 | 1770 | 161.2 |
| photo | Tesseract / ron | 1.49% | 8.33% | 3177.5 | 1150 | 55.3 |
| photo | ocrs / default | 11.94% | 50.00% | 3050.6 | 1660 | 161.5 |

Across all four fixtures, character-weighted CER is **3.35% for Tesseract**
and **12.57% for ocrs**. The clean Romanian sample is 0.96% versus 20.19%.
These are tiny synthetic samples, not a general OCR benchmark.

The laptop was heavily loaded during measurement: one-minute load average
was 59.8 before and 53.1 after. The raw per-run wall times vary widely.
Therefore wall-time rankings are inconclusive; CPU time is included to separate
compute cost from scheduling delays. These are **not Fairphone 6 latency or
battery measurements**. RSS is GNU time’s process/child high-water mark, not the
sum of all app, service, and QML processes. The 384 MiB service cap has not been
stress-tested on the phone.

## Method and reproduction

- Rendered only synthetic strings with Pillow / DejaVu Sans 2.37 at 38 px on
  1100 × 320 RGB images. No user screenshots, documents, or photos were used.
- English, Romanian, and mixed-language screenshots; one Romanian receipt-like
  image rotated 2 degrees, blurred by 0.45 px and JPEG-compressed at quality 85.
- Three separate process runs per engine/case; median wall and user+system CPU
  time, and maximum RSS across the three runs. Both engines use one compute
  thread (`OMP_THREAD_LIMIT=1`, `RTEN_NUM_THREADS=1`, `RAYON_NUM_THREADS=1`).
- Tesseract goes through the production Rust preprocessing/TSV path, PSM 3,
  using the language shown in the table. ocrs uses `prepare_input` / `get_text`
  and loads both models for every process. This compares cold-process workflows;
  retaining ocrs models across calls could improve its latency.
- CER/WER use Levenshtein distance on NFC-normalized text / whitespace-delimited
  words after collapsing whitespace. Case, punctuation, and diacritics count.
  Formatting and line-break differences do not count.
- Exact recognized text and per-run timings: [results.json](results.json).
- Scripts: `scripts/render_samples.py`, `scripts/fetch_models.sh`,
  `examples/evaluate_ocrs.rs`, and `scripts/evaluate.py`. Commands are in README.
- Tesseract production binary was the successful Nix release build with Rust
  1.99.0. The ocrs release harness uses stable Rust 1.95.0. Nix Rust 1.99.0
  reported an AVX-512 intrinsic signature mismatch in RTen; 1.96.1 reported
  an LLVM instruction-selection error. Rust 1.95.0 compiled and ran the
  unmodified dependencies. Default production builds do not include RTen.

## Model sources and integrity

The fetch script downloads these upstream model assets only on explicit request:

- `https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.onnx`
- `https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.onnx`

SHA-256 values are pinned in [models.sha256](models.sha256), verified by the
fetch script. Models are ignored by Git and are not part of the shipped app.
Crates are pinned by Cargo.lock. Tesseract language data come from the nixpkgs
Tesseract override with `enableLanguages = [ "eng" "ron" ]` (tessdata 4.1.0
in the checked nixpkgs revision). No runtime download exists.

## Quality limits

Tesseract confused `pâine` with `păine` once in the clean Romanian fixture and
lowercased `Pâine` in the synthetic photo. Mixed mode lost diacritics and
introduced spurious isolated characters; 12.20% CER is not acceptable for
unreviewed transcription. Explicit Romanian mode is recommended for Romanian
images. Reversing bilingual language order did not reliably fix the issue.

The default ocrs model replaced many accented letters with `?` or unaccented
letters. A multilingual recognition model would need a separate quality and
resource evaluation before replacing Tesseract. Neither this experiment nor
the app promises reliable handwriting, perspective correction, tiny text, or
complex real-world photos. The editable text area supports manual correction.
Real-phone test steps are in README.
