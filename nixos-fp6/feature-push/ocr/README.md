# Text from images

Offline OCR for Fairphone 6 / aarch64 NixOS with Lomiri, plus a standalone Rust
CLI. English and Romanian language data are bundled. No account, telemetry, cloud
OCR, or runtime downloads. PNG and JPEG inputs are supported, including EXIF
orientation. Recognition runs in the background with stage progress and cancel.

## Build and run

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-build
./result/bin/fairphone-ocr recognize image.png
./result/bin/fairphone-ocr recognize image.jpg --lang ron --json
```

`--lang` accepts `eng`, `ron`, `eng+ron` (default), or `ron+eng`. JSON includes
original oriented image dimensions, word rectangles, confidence, text and timing.
The direct CLI does not use D-Bus or write an index. For Romanian-only images,
choose `--lang ron` (or **Română** in the app): bilingual mode can favor English
spellings and drop diacritics. All OCR output should be reviewed before reuse.

Install the NixOS module:

```nix
{
  imports = [ ./path/to/ocr/module.nix ];
  hardware.fairphone6.ocr.enable = true;
  # Adjust this to the screenshot tool's actual directory if needed:
  hardware.fairphone6.ocr.screenshotDirectory = "%h/Pictures/Screenshots";
}
```

The module installs the desktop launcher, Content Hub descriptor, session D-Bus
activation file and `fairphone-ocr.service` user service. It also registers the
Content Hub package and its D-Bus activation files. The native peer descriptor
is installed as `share/lomiri-content-hub/peers/fairphone-ocr`, without a suffix. Open **Text from images**
or run `fairphone-ocr ui /absolute/path/image.png`. Selecting the desktop app as an
image handler also opens a local image. There is no recursive scanning: **Latest
screenshot** selects the newest PNG/JPEG in the configured directory.

For development without installing the module:

```sh
nix-shell
cargo test
cargo build
# In a terminal inside the desktop's session:
target/debug/fairphone-ocr serve
# In another terminal in the same session:
QT_QPA_PLATFORM=wayland target/debug/fairphone-ocr ui image.png
```

The development shell defaults to offscreen rendering with Mesa software OpenGL for reproducible
previews (UITK shapes require OpenGL, so do not set `QT_QUICK_BACKEND=software`).
On a host without a display, provide an Xvfb display for the offscreen plugin's
GLX context. Choose `wayland` or `xcb` for an interactive desktop window. If the
sandbox makes your Cargo cache read-only, set `CARGO_HOME=/tmp/fp6-ocr-cargo`.
`OCR_SCREENSHOT_DIR` configures the service directory outside systemd.

## App workflow

- **Open image** chooses a source through Content Hub. Incoming image shares also
  start recognition. A local path field and the CLI work without Content Hub.
- Choose English, Romanian, or both before starting recognition.
- Tap word boxes to select multiple regions. **Copy selected** uses those regions
  in reading order, or the selection in the text area. **Copy all** copies the
  editable text area. Editing recognized text does not alter the image boxes.
- **Share** sends the selected regions/selected text, or all text, as UTF-8 through
  the Content Hub share picker. Share files last until the window closes.
- **Remember text on this device** explicitly enables the local text index.
  Search matches case-insensitively with Unicode NFC normalization; accents remain
  significant. Tap a result to reopen its source image. A missing image produces
  an error. Disable the switch to delete all indexed text and paths.

The index retains at most 500 images and the first 8,000 characters of each,
returning up to 50 matches. It is a small JSON substring index, not semantic
search. It does not add existing screenshots retroactively. Storage defaults to
`$XDG_DATA_HOME/fairphone-ocr/index.json`, otherwise
`~/.local/share/fairphone-ocr/index.json` (directory 0700, file 0600). Files are
written atomically. Recognition still succeeds if an index write fails, with a
visible warning. Disabling indexing is only acknowledged after the purge is saved.

Images over 24 MiB or 12 megapixels are rejected before decoding. Larger accepted
images are downscaled to a 2,400 pixel longest edge for OCR. Tesseract uses one
thread, with a 90 second deadline including preparation. Four queued jobs and one
active job are allowed. The service retains up to 32 jobs for at most approximately
15 minutes. The systemd service has a 384 MiB memory cap and one CPU's quota. These
are safeguards, not measured phone performance guarantees.

## D-Bus API

Session bus name and interface: `com.fairphone6.Ocr1`.
Object: `/com/fairphone6/Ocr1`. Methods use D-Bus errors for invalid requests.

| Method | Input | Output |
|---|---|---|
| `Recognize` | `s path, s languages` | `s job_id` |
| `GetJob` | `s job_id` | `s JSON` |
| `CancelJob` | `s job_id` | none |
| `LatestScreenshot` | none | `s local_path` |
| `GetSettings` | none | `s JSON` |
| `SetIndexEnabled` | `b enabled` | none |
| `Search` | `s query` | `s JSON array` |

```sh
busctl --user call com.fairphone6.Ocr1 /com/fairphone6/Ocr1 \
  com.fairphone6.Ocr1 Recognize ss /home/user/Pictures/example.png eng+ron
busctl --user call com.fairphone6.Ocr1 /com/fairphone6/Ocr1 \
  com.fairphone6.Ocr1 GetJob s JOB_ID
```

Jobs have `queued`, `running`, `done`, `failed`, or `canceled` status, stage,
progress (0–100), nullable result/error/warning. Progress represents preparation,
recognition, and parsing stages; Tesseract does not provide a true percentage.
Clients should poll at about 500 ms and stop on terminal status. Expired IDs
produce an error. Coordinates are oriented source-image pixels, not screen pixels.
Only local paths and local `file://` URLs are accepted. The session bus is the
same-user trust boundary; this service is not a sandbox for untrusted local apps.

## Verification

```sh
nix-shell --run 'cargo test && cargo clippy --all-targets -- -D warnings'
nix-shell --run 'python3 scripts/render_samples.py'
nix-shell --run 'dbus-run-session -- python3 scripts/integration.py'
nix-shell --run 'dbus-run-session -- python3 scripts/package_smoke.py ./result'
nix-shell --run 'dbus-run-session -- python3 scripts/content_hub_smoke.py'
nix-shell --run 'python3 scripts/preview.py empty'
# Evaluation fetches only the upstream model files:
nix-shell --run 'bash scripts/fetch_models.sh'
nix-shell --run 'cargo build --release --features evaluate-ocrs --example evaluate_ocrs'
nix-shell --run 'cargo build --release && python3 scripts/evaluate.py'
nix-shell --run 'python3 scripts/preview.py recognized'
```

The `ocrs` dependency is optional and excluded from production builds. The
benchmark was built with stable Rust 1.95.0: this laptop's Nix Rust 1.99.0/1.96.1
LLVM builds fail on an AVX-512 intrinsic in RTen 0.26.0. If affected, use
`RUSTC=/path/to/rustc-1.95.0 cargo build --release --features evaluate-ocrs --example evaluate_ocrs`
(or `cargo +1.95.0 ...` with rustup). The production package and default tests
work with the normal Nix stable toolchain. See
[evaluation/REPORT.md](evaluation/REPORT.md) for model hashes, measured accuracy,
speed, memory and benchmark/toolchain caveats. All fixtures and previews are
synthetic. See [DESIGN.md](DESIGN.md) for tradeoffs and trust boundaries.

## Manual checklist on the real phone

No phone or remote host was accessed during development. Check these on-device:

- Enable the module; verify `systemctl --user status fairphone-ocr` and D-Bus
  activation after stopping the service. Confirm both language packs are present.
- Launch from Lomiri and from gallery **Open with** / **Share**. Import a picture
  through Content Hub. Confirm cold-launch and already-running imports.
- Open a screenshot using the CLI and **Latest screenshot**, including a filename
  with spaces and Romanian diacritics. Adjust the screenshot directory if needed.
- Recognize English, Romanian (ă â î ș ț and capitals) and mixed text. Compare
  portrait JPEGs with EXIF rotation, low contrast photos and blank images.
- Tap small boxes with a finger, select multiple words, edit/copy text, paste into
  another app, and share to a real text-capable Content Hub destination. Keep the
  OCR window open until the receiving app has collected the share.
- Check Maliit keyboard, scrolling, rotation, Suru theme, and 360–480 logical-pixel
  layouts. Verify keyboard dismissal and clipboard behavior across app switches.
- Cancel running/queued jobs; check malformed images, oversized images, absent
  files, missing language data and a full queue. The UI should remain responsive.
- Confirm no index file after recognition with indexing off. Enable, recognize,
  restart the service, search `ROMÂNIA`, disable, restart, and verify purge.
- Confirm airplane-mode operation. Observe service CPU/RSS and battery use on
  representative 12 MP photos; inspect OOM/restart behavior under memory pressure.
- Verify that session logout closes the UI and service and that temporary files
  are removed; check Content Hub's own retention policy for imported files.
