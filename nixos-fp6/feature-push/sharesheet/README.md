# fp6-share

A unified Rust + Qt 5.15 / Lomiri UI Toolkit 1.3 share sheet for Fairphone 6
on Mobile NixOS. Choose an installed app, a nearby device, a temporary WebShare
link/QR, or a local action. Recent devices appear first in their own row.
The sheet sends only after a target is selected. No telemetry or address-book access.

## Build and verification

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell --run 'cargo test --locked'
nix-shell --run 'cargo build --features ui --locked'
nix-shell --run 'cargo fmt --check && cargo clippy --all-targets --features ui --locked -- -D warnings'
nix-shell --run 'scripts/ui-smoke.sh'
nix-shell --run 'scripts/preview.sh'
nix-instantiate --eval --strict --expr 'import ./tests/module.nix {}'
nix-build
nix-shell --run 'scripts/package-smoke.sh'
# Exercise the installed wrappers and QML, too:
nix-shell --run 'FP6_SHARE_BIN_DIR=$PWD/result/bin scripts/ui-smoke.sh'
nix-shell --run 'FP6_SHARE_UI_BIN=$PWD/result/bin/fp6-share-ui scripts/preview.sh'
```

Default Cargo builds need stable Rust and `dbus-run-session` for the integration
test; they do not require Qt. `--features ui` adds the qmetaobject bridge and UI
binary. `shell.nix` supplies the compiler, Qt, UITK, Content Hub and Xvfb; its
Cargo cache is under `/tmp/fp6-share-cargo`. `Cargo.lock` pins dependencies.
`package.nix` uses `rustPlatform.buildRustPackage` and runs tests on a private bus
inside the Nix sandbox. aarch64 module evaluation is checked; a native aarch64
build and phone interoperability remain manual checks.

Tests use synthetic inputs and isolated session services. They do not discover
real devices, contact a phone, print a real job or use the actual clipboard.
Previews use synthetic provider fixtures without connecting to the share daemon.
PNGs are in [previews/](previews/): image, text, unavailable providers, and error.
The UI scrolls on a phone; previews use a taller canvas to show all rows.
Offscreen rendering uses `QT_QPA_PLATFORM=offscreen`, Xvfb and Mesa software
OpenGL (`LIBGL_ALWAYS_SOFTWARE=1`); UITK shapes require OpenGL. Leave
`QT_QUICK_BACKEND` unset.

## Install on NixOS

```nix
{
  imports = [ ./sharesheet/module.nix ];
  hardware.fairphone6.share.enable = true;
}
```

This installs the desktop entry (`X-Lomiri-Touch=true`), Content Hub share peer,
D-Bus activation file and **systemd user** service `fp6-share`. It registers
`share:` as a MIME scheme handler. It does not enable the optional transfer
services or open firewall ports. Enable the desired sibling modules separately;
WebShare also needs its executable on the system profile. The service PATH
includes `/run/current-system/sw/bin` so installed providers can be launched.

The existing Lomiri session must export its display, Mir/Wayland and session-bus
environment to the systemd user manager. The package supplies QML imports and a
default APP_ID. Content Hub must be running/activatable in that session; restart
the session after first installing peer files. Nixpkgs patches its native peer
search to `/run/current-system/sw/share/lomiri-content-hub/peers`. The module
links that directory into the system profile.

```sh
systemctl --user start fp6-share
systemctl --user status fp6-share
journalctl --user -u fp6-share
fp6-share ./example.png ./notes.pdf
fp6-share --text 'A synthetic invitation'
fp6-share --url https://example.invalid/invitation
fp6-share 'share:?text=Hello%20from%20an%20app'
fp6-share --clear-history
```

For development, run `target/debug/fp6-share daemon` within `nix-shell`; the GUI
binary must be built alongside it. `fp6-share-ui` opens an idle sheet that can
receive Content Hub requests; it also accepts the same item arguments as the CLI.
A second request is rejected while an existing request is open. Close the sheet
before starting another share.

## Apps, portals and Content Hub

There is no standard Share portal. This package uses a desktop MIME handler,
leaving the existing `org.freedesktop.portal.Desktop` service intact. GTK/Qt apps
can open `share:?text=...` or `share:?url=...` through their URI-opening API;
sandboxed apps can use the real portal's OpenURI method with the same URI.
Values must be UTF-8 percent encoded. One parameter is allowed; `url` accepts
only HTTP(S). Browser-originated `share:` requests cannot name local files.
Literal `file:` text is rejected as ambiguous; use file arguments for local files.

For local files, choose Share in a file manager's **Open With** menu, or explicitly
choose the Share desktop entry through the real portal's OpenFile app chooser.
The desktop entry advertises common image, PDF, text, audio, video and binary MIME
types. Only the `share:` scheme becomes a default handler. Sandboxed callers must
supply files exported through their existing document/file portal; this sheet
does not grant access to private sandbox paths. Firefox can open the custom
scheme with the normal external-app consent prompt. This does not add a native
Share button or implement `navigator.share()` in Firefox. See [DESIGN.md](DESIGN.md)
for the compatibility research and an OpenURI call example.

UT apps see **Share** through the Content Hub share registry. All nine concrete
Content Hub types are registered; `share: ["all"]` is not supported upstream.
The sheet queries `KnownSharesForType` and hands selected items to
`Lomiri.Content`'s `ContentPeer.request()`, preserving transfer objects until the
receiving app collects the items. Keep the sheet open during that handoff.

## Providers and local actions

| Target | Behavior |
| --- | --- |
| Quick Share | Calls StartDiscovery only while a sheet is open, lists devices, sends local file URIs or a single text/link with the sibling API's prefixes; shows status/PIN and supports Cancel. Visibility is unchanged. |
| KDE Connect | Shows online, paired devices with the share plugin enabled. Sends local files or an HTTP(S) link using `Action(id,"share",item)`. Plain text is hidden because this sibling API does not accept it. Multi-file dispatch is sequential; a later failure can follow earlier successful items. |
| Share via link / QR | Opens the installed WebShare UI to choose Wi-Fi, expiry and show its QR. Text and links become a temporary UTF-8 `.txt` download in a private runtime file. No listener starts until the user starts sharing there. |
| Content Hub apps | Registry filtered by content type; excludes this sheet to prevent a routing loop. |
| Copy | Copies text, links or newline-separated file URIs using the session's Qt clipboard. |
| Save to Files | One item; calls the real FileChooser SaveFile portal, subscribes before the Response, supports cancellation and streams local files. Existing destinations, including symlinks, are preserved: choose an unused filename. No fallback dialog when the portal is absent. |
| Print | Visible for supported local file types when CUPS tools and a default printer are available. Requires a second confirmation; submits to that printer with `lp`, without a shell. Queued does not mean physically printed. |
| Text recognition | One local image; calls Ocr1 Recognize with `eng+ron`, shows editable recognized text, Copy and Cancel. No image upload or automatic indexing. |

Missing or failing providers are hidden independently. Optional services must
already own their bus names; the sheet does not activate every installed service
just to enumerate targets. Devices refresh every two seconds and on Quick Share
signals. Selection rechecks availability. Nearby discovery is stopped on sheet
close only when this service started it from idle. Provider transfer policies,
limits and confirmation dialogs remain authoritative.

The sheet accepts 1–32 regular local files, or text/link strings totaling at most
128 KiB (URI bytes, not file contents). Text and files cannot be mixed in one
request. File MIME detection is extension/category based, not a decoder; each
provider still validates its own inputs. No remote file downloads are performed.

Only the last eight **device identifiers** are persisted, in
`${XDG_STATE_HOME:-~/.local/state}/fp6-share/recent.json` (directory 0700, file
0600). No labels, content, file paths, contacts or timestamps are saved there.
Unavailable devices are omitted from the recent row, then restored when available.
Clear history in the sheet or CLI. The supplied APIs expose devices rather than
individual people, so no invented person-level destinations are shown. Shared
payloads otherwise remain in memory. WebShare's temporary text file is mode 0600,
held until its GUI exits, then removed; its daemon keeps open file descriptors
for an already-started session. Clipboard and chosen destinations have their
normal lifetimes. Restarting this daemon discards the active share.

## Manual checklist on the real phone

All checks below are pending. Use consenting test devices and synthetic files.

- [ ] Build/install natively for aarch64; enable the module and verify the user
  service, desktop icon, Content Hub peer, APP_ID and QML imports under Lomiri.
- [ ] Test 360–480 pixel portrait and landscape layouts, scrolling, long labels,
  touch controls, Maliit in OCR's text editor and reopening a minimized sheet.
- [ ] Share one/multiple images, documents, contacts, events, music, video, ebooks,
  text and links from UT apps; test cold launch and an already-open idle sheet.
  Select a receiving app and confirm collection before closing the sheet.
- [ ] Try CLI paths with spaces and Romanian letters, file URIs, text, a URL,
  missing files, directories, mixed/oversized requests and a second active request.
- [ ] Test GTK/Qt Open With and OpenFile chooser; from a sandbox use exported
  files. Test OpenURI `share:?text=...` and Firefox's custom-scheme consent.
  Confirm that a `share:` URI cannot load a local file.
- [ ] Enable Quick Share, discover a consenting receiver, compare PINs, transfer
  files/text/links, cancel and disconnect mid-transfer. Check discovery stops
  after closing and does not change receiver visibility.
- [ ] Enable KDE Connect; show only paired, online, share-enabled peers. Send
  files and HTTP(S); disable share/unpair/disconnect between display and selection.
- [ ] Open WebShare for files and text/link downloads, choose Wi-Fi/expiry, scan
  its QR, download and compare bytes, stop/expire, and confirm runtime text-file
  cleanup after its GUI exits. Its transport is HTTP; use a trusted test LAN.
- [ ] Copy/paste text and file URIs; SaveFile cancel/success/collision/symlink/full
  disk; ensure files are not overwritten. Test a document-portal destination.
- [ ] With no CUPS/default printer, Print is hidden. With a synthetic test queue,
  decline confirmation, then confirm and check only the selected file is queued.
- [ ] OCR an English/Romanian image, edit/copy text, cancel, fail a job, and stop
  the OCR service during polling. Confirm no indexing is enabled by this sheet.
- [ ] Use more than eight devices, repeat a destination, restart, clear history,
  and inspect private permissions and identifiers-only persistence.
- [ ] Stop/restart each optional provider and the share service, close the UI
  immediately before another CLI share, then test suspend/resume and logout.
  Confirm no payload logging, unwanted listener, or stale content after close.

## Patch series

The workspace's `.git` is read-only. Small implementation commits are exported
as numbered patches under [patches/](patches/). On a clean checkout at the task
brief commit, apply in order with `git am patches/0*.patch` (keep the patch files
outside that checkout if needed). The working tree already contains the changes;
do not apply them over these same files. See [FINAL_REPORT.md](FINAL_REPORT.md)
for final validation and remaining phone checks.
