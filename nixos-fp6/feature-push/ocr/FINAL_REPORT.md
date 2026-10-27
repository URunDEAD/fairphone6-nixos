# Final report

Completed 2026-10-27. The repository contains an offline Rust OCR tool, session
D-Bus service, Lomiri QML app, Nix packaging/module, reproducible synthetic engine
evaluation, automated checks, previews, and phone validation instructions.

## Delivered

- CLI: `fairphone-ocr recognize IMAGE [--lang eng|ron|eng+ron] [--json]`.
- Lomiri app: command-line image opening, Content Hub image import/share target,
  latest screenshot, selectable word regions, editable text, copy all/selected,
  text sharing, and opt-in screenshot text search.
- `com.fairphone6.Ocr1` session D-Bus API with queued background recognition,
  stage progress, cancellation, settings, screenshot lookup, and search.
- English and Romanian Tesseract language data bundled by Nix. No runtime model
  downloads, telemetry, or cloud OCR. Application Rust forbids unsafe code.
- Input/queue/history limits, single-threaded engine execution, deadline,
  private temporary files, expiring results, and atomic private index storage.
  Disabling the index purges its text and paths. OCR-derived UI text is plain text.
- `package.nix` uses `rustPlatform.buildRustPackage` for the tested Rust core and
  a separate runtime packaging derivation for QML/desktop assets.
- `module.nix`: `hardware.fairphone6.ocr.enable`, user service, D-Bus and Content
  Hub activation, native peer registration, CPU/memory limits, private runtime
  scratch directory, and `X-Lomiri-Touch=true` desktop launcher.
- [README.md](README.md): setup, API reference, reproduction commands, and a real
  phone checklist. [DESIGN.md](DESIGN.md): architecture and privacy boundaries.

## Verification results

| Check | Result |
|---|---|
| `cargo test` through the Nix development shell | PASS: 7 tests |
| `cargo clippy --all-targets -- -D warnings` | PASS |
| `cargo fmt -- --check` | PASS |
| Native `nix-build` with `NIX_PATH=nixpkgs=channel:nixos-unstable` | PASS, including all 7 release tests |
| NixOS module evaluation, x86_64 and aarch64 | PASS |
| Live D-Bus / OCR / index opt-in and purge / cancellation integration | PASS |
| Actual QML + local bridge + D-Bus + OCR + clipboard selection | PASS |
| Content Hub receiver event ordering and duplicate suppression | PASS |
| Packaged CLI, metadata, service, and actual `qmlscene` launcher | PASS |
| Native Content Hub image destination and share-target registry queries | PASS |
| Model SHA-256 checks and Git whitespace checks | PASS |

Final native package output:
`/nix/store/7dhcmx7gjchql4vral980l82vpkvdzsj-fairphone-ocr-0.1.0`.
The local `result` symlink points to it. Nixpkgs was
`26.11pre1100280.3ec636362e18`; production Rust was stable 1.99.0.

Actual Lomiri UITK 1.3 previews are saved in:

- [previews/empty.png](previews/empty.png)
- [previews/recognized.png](previews/recognized.png)
- [previews/live-recognition.png](previews/live-recognition.png)

They were rendered with `QT_QPA_PLATFORM=offscreen` and Mesa software OpenGL.
UITK's shape renderer requires OpenGL; Qt Quick's `software` scenegraph backend
is unsuitable. Some theme-icon warnings occur in the minimal laptop environment.
No mocked UITK controls were substituted.

## Engine decision

Both Tesseract 5.5.3 and `ocrs` 0.13.1 / RTen 0.26.0 were run on four rendered
synthetic fixtures, three cold-process runs each, with one compute thread.

Tesseract is selected. Character-weighted error was **3.35%**, versus **12.57%**
for ocrs. On clean Romanian text it was **0.96% versus 20.19%**. Measured maximum
process/child RSS ranged from about **55–103 MiB** for Tesseract and **161 MiB**
for ocrs. Tesseract also used less CPU time on these samples.

Wall-clock timing was affected by high laptop load; the report includes wall
and CPU measurements and every run. These are not phone performance estimates.
See [evaluation/REPORT.md](evaluation/REPORT.md) and
[evaluation/results.json](evaluation/results.json).

The optional ocrs benchmark required stable Rust 1.95.0 on this laptop because
its newer Nix Rust/LLVM toolchains failed on an RTen AVX-512 intrinsic. Production
builds and default tests do not depend on that optional runtime.

## Practical limits and remaining device validation

Romanian-only mode is recommended for Romanian images. Bilingual recognition
can drop diacritics and produced 12.20% character error on the mixed synthetic
fixture. The text editor permits correction; handwriting, strong perspective,
and complex photos are not guaranteed.

No phone or remote host was accessed. ARM execution, real gallery-to-app and
app-to-recipient Content Hub transfers, Maliit behavior, touch ergonomics,
real-camera accuracy, and phone battery/memory performance remain the explicit
manual checklist in README. Native registration, receiver event handling, and
packaged application launch were tested locally. Content Hub manages its own
imported copies; exported text must be collected before the OCR window closes.

## Commit delivery

The workspace policy mounts the original `.git` directory read-only. Creating
`.git/index.lock` failed, so its metadata could not be updated. All implementation
files are present in the requested directory. Small commits were recorded in a
separate Git directory and supplied in **`commits.bundle`**, including the original
task brief and this final report.

To obtain a normal checkout with the complete history without replacing this
working directory:

```sh
git clone ./commits.bundle ../ocr-completed
```

No commits were pushed and no system configuration was deployed.
