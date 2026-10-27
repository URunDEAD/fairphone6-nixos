# Design

The service owns a single OCR worker and a bounded, ephemeral job history. Tesseract
runs as a child process with one OpenMP thread, an execution deadline, and image
size limits. This isolates the native engine without unsafe Rust or a native FFI
build dependency. Engine choice will be validated against ocrs on synthetic data.

The session D-Bus service `com.fairphone6.Ocr1`, object `/com/fairphone6/Ocr1`,
provides asynchronous job submission and polling for the app, screenshot tool,
and gallery. The CLI can also recognize an image directly without a session bus.

The Qt 5 / Lomiri UI Toolkit app displays image-relative word boxes and editable,
selectable text. A per-window Rust bridge forwards QML JSON requests to D-Bus over
an ephemeral loopback port protected by a random bearer capability. It exists
only while that QML window is open; no external network traffic is needed.

Recognition is ephemeral by default. The opt-in local index stores bounded text
and canonical image paths in private user data files. Disabling indexing purges
entries. There is no telemetry, network OCR, automatic scanning, or model download
at runtime. Content Hub is used only for explicit image import and text sharing.

## Coordinates and process boundary

Input is snapshotted into a private temporary directory using a bounded read.
Only PNG/JPEG signatures are accepted. Header dimensions are checked before full
decode, EXIF orientation is applied, and the longest edge is limited to 2,400
pixels. The prepared RGB PNG goes to Tesseract with `--psm 3`. TSV word rectangles
are scaled back to oriented source coordinates. QML uses `autoTransform` and the
painted image rectangle, preserving letterboxing alignment. This supports mixed
layouts better than hardcoding a single text block. Handwriting and reliable
perspective correction are outside this version.

A subprocess was chosen over leptess/tesseract-rs because it gives cancellation,
a hard recognition deadline, language isolation, and simpler aarch64 packaging
without unsafe application code. The engine still uses the native Tesseract and
Leptonica implementation supplied by nixpkgs. Both language packs are installed
at build time. No executable or model is downloaded by recognition requests.

TSV confidence is retained for clients; the app does not silently discard low
confidence words. Selecting boxes follows Tesseract's reading order and line IDs.
The text area is editable so errors can be corrected before copying/sharing.
All labels and text editors explicitly use plain text: recognized HTML-like
content cannot create rich-text image loads.

## Concurrency and progress

D-Bus handlers only submit work or access small state objects. One standard Rust
thread performs decode and OCR; a bounded channel allows four waiting jobs.
Cancellation marks a job immediately and the worker kills/reaps the child at its
next 100 ms check. Queued canceled jobs are skipped. A separate expiry sweep
removes terminal jobs after 15 minutes, at 30 second intervals, even when idle.
History is also pruned on access. Percentages are stage indicators, not a claim
about Tesseract's internal completion. No text or image payload is sent to journald.

The user service limits CPU and memory and only allows Unix sockets. Its OCR
snapshots live in a 0700 systemd runtime directory that is removed when the
service stops. It intentionally shares the caller's filesystem view, including
`/tmp`, so images supplied from temporary directories remain readable. The UI
bridge is a separate short-lived process with a loopback listener. QML is
launched by Rust using a private temporary entrypoint with a random UUID
capability URL/header. Only POST with both the correct URL and custom header is
accepted; no CORS response is provided. JSON requests are capped at 128 KiB.
The bridge calls the public D-Bus API. It is not a second persistence owner.
Closing the window removes exported text and bridge files. Same-user applications
that can inspect process files/session D-Bus are trusted; neither interface is
intended to defend against an already compromised user session.

## Persistence and sharing

A single service owns the index. A temporary 0600 file is synced and renamed over
`index.json`; an unsuccessful save leaves in-memory settings unchanged. Disabled
indexing stores no recognition text. Purging replaces the file with disabled,
empty state. This is logical deletion, not a secure erase guarantee on flash or
backups. Query normalization preserves Romanian accents and unifies composed /
decomposed Unicode forms. A bounded JSON index avoids another database dependency
for a maximum 500 entries; larger collections would warrant SQLite FTS.

Content Hub is isolated in a Loader, so the local path workflow remains available
if its QML module is absent. Imports use the hub's app-scoped store and keep the
transfer alive while the worker snapshots the file. Hub-managed copies follow
Content Hub's own retention policy; the OCR app never deletes a user's source
image. Explicit shares write UTF-8 files in a private per-window directory and
pass both file URL and text to a ContentItem. The receiving app must collect the
share before the source window closes. Real import/export lifecycle behavior
requires the phone checklist, beyond a local component/load smoke test.

## Rendering and deployment

The UI uses actual Lomiri.Components 1.3 controls and Qt 5.15, with label spacing
provided by anchors/margins. There are no GSettings schemas or unsupported ai/ax
keys. The theme icon dependency comes from the Lomiri desktop; missing icons on a
minimal development session do not replace the actual controls.

The offscreen preview uses `QT_QPA_PLATFORM=offscreen` and Mesa software OpenGL.
Qt Quick's `software` scenegraph backend is unsuitable for this version of UITK:
LomiriShape assumes a current OpenGL context. This was verified by a stack trace
and resolved by using software OpenGL, without substituting mocked UI components.
Python/PyQt5 is used only for synthetic rendering, screenshots and integration
checks; all application code and the bridge are Rust/QML.

The Nix module supplies a systemd user service and D-Bus activation, with no root
runtime process and no phone-specific hardware access. The native x86_64 build
runs tests; the module/package also evaluate for aarch64. Execution on ARM and
real Lomiri/Content Hub/Maliit interaction are manual validation items.

Native Content Hub registration uses an extensionless app-ID file under
`share/lomiri-content-hub/peers`, linked into the system profile by the module.
Receiver logic accepts Charged or Collected transfers and waits for items when
state notifications arrive first, with duplicate suppression. The dedicated
receiver smoke test exercises these event orderings.
