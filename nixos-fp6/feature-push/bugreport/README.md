# fp6-bugreport

Local diagnostics for the Fairphone 6 running aarch64 NixOS / Mobile NixOS and Lomiri. A Rust CLI and Qt 5.15 / Lomiri UI Toolkit 1.3 app collect selected diagnostics, redact text and create a private `tar.zst` plus a readable summary. Nothing is uploaded. Screenshots and privileged collection are **off by default**.

## Build and test

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
# If your home directory is read-only, also set CARGO_HOME to a writable directory.
nix-shell --run 'cargo test --locked'
nix-shell --run 'cargo build --locked --features gui'
nix-shell --run './scripts/preview.sh'
nix-build -E '(import <nixpkgs> {}).callPackage ./package.nix {}'
nix-instantiate --eval --strict -E 'import ./tests/module-eval.nix {}'
```

On other Linux systems, `cargo test --locked` needs stable Rust and a C compiler (zstd). Qt is only needed for `--features gui`; provide Qt 5 Core/Gui/Qml/Quick development files and `qmake`. Running the GUI also needs the actual `Lomiri.Components 1.3` QML module, not a substitute. The Nix package includes those runtime dependencies and fixed paths for diagnostic tools. `withGui = false` builds a CLI/helper package.

Previews under `previews/` are produced by the real Rust/QML app with `QT_QPA_PLATFORM=offscreen` and Mesa software OpenGL (UITK shape nodes require OpenGL). On a headless machine the script uses Xvfb to supply GLX while retaining the offscreen QPA platform. Preview mode disables collection, including screenshots and privilege requests.

## Install on the phone

Import this repository's module into your phone configuration:

```nix
{
  imports = [ /path/to/fp6-bugreport/module.nix ];
  hardware.fairphone6.bugreport.enable = true;
}
```

Build/deploy using your normal phone configuration workflow. The module installs **Create bug report**, a touch-enabled application in the Settings/System launcher categories, and an on-demand `fp6-bugreport.service` **user** unit. There is no automatic collection at login. The app is a standalone Settings-category entry, not an in-process Lomiri System Settings plugin. The desktop entry starts the user unit so repeated launches cannot create multiple collector windows. The running app can be restored through Lomiri's app switcher.

The Lomiri session must import `DISPLAY` or `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, and `DBUS_SESSION_BUS_ADDRESS` into the systemd user manager, as it does for other graphical user services. A functioning session polkit agent is needed only for the optional restricted logs. The helper has no setuid bit and the policy requires active-session administrator authentication for each request. Inactive and remote requests are denied by policy.

The package alone can run the CLI or `fp6-bugreport-gui` directly. Its desktop entry requires the module's user service.

## CLI

```sh
fp6-bugreport --preview
fp6-bugreport --since 1h --sections system,units,journal,power,memory
fp6-bugreport --since 15m --sections kernel,display --privileged
fp6-bugreport --capture-now --since 15m --sections display,journal,memory
# Explicit consent to an UNREDACTED image:
fp6-bugreport --capture-now --screenshot --sections display,journal
```

`--since` accepts whole minutes (`m`), hours (`h`) or days (`d`), from `1m` through `7d`. It applies to journal queries and the coredump list; dmesg is its current bounded ring buffer. All collection is immediate; `--capture-now` enables the explicit screenshot opt-in. There is no historical screenshot, background recorder or persistent log watcher.

`--sections` is a comma-separated nonempty subset of:

| Section | Sources |
|---|---|
| `system` | `/etc/fp6-release`, `/proc/uptime`, `/proc/version` |
| `units` | Failed system and user units |
| `journal` | Recent user and accessible system journal, at most 3,000 lines each |
| `kernel` | Current dmesg ring buffer, if readable |
| `coredumps` | `coredumpctl list` metadata only; never core contents |
| `power` | Allowlisted battery and thermal sysfs properties |
| `modem` | oFono service status; typed allowlist of powered/online/SIM/registration state |
| `network` | Link, IPv4/IPv6 address and route state; NetworkManager connectivity flags |
| `audio` | `wpctl status` |
| `display` | Recent GPU/display warnings and shell/rendering journal messages |
| `memory` | meminfo, vmstat, memory pressure, zram0 stats, app-ready units and OOM events |
| `storage` | Filesystem usage, without enumerating user files |

Optional tools, missing devices, restricted journals, denied authentication, and a missing screenshot API are recorded as unavailable. Successful archive creation can include partial data; read `summary.txt`. A zero exit status means a report was saved, not that every source was readable or the device is healthy.

## Output and privacy

Reports are saved to:

```text
~/Documents/Bug reports/fp6-<epoch>-<random>/
    report.tar.zst     # 0600
    summary.txt        # 0600; also inside the archive
```

Both the reports root and each report directory are `0700`. An existing insecure, foreign-owned or symlinked `Bug reports` directory is rejected instead of silently changing its permissions. Each report has a unique directory, so reports cannot overwrite each other's summaries. The literal `~/Documents` is used, independently of localized XDG directory names.

Text is redacted in memory and again when written. Rules mask international/domestic telephone numbers (including +40, 07xx and +999 test numbers), long numeric device IDs, IMEI/ICCID/IMSI fields, MAC/BSSID, IPv4/IPv6, SSID-bearing lines, email, common GPS forms, WireGuard base64 keys, JWTs, and secret-bearing assignments such as passwords, tokens and NetBird setup keys. False positives are intentional: long numeric counters and sensitive-field lines may be lost.

Logs can contain arbitrary human text, unusual encodings, app-specific credentials, or names without labels. Regex redaction is not a proof of anonymity. Review the summary and extracted content locally before sharing. Screenshots are **never** text-redacted or OCR-scrubbed. No archive is sent anywhere, and there is no telemetry or network client in the collector.

Inspect a copy locally with `tar --zstd -tf report.tar.zst` and extract into a private directory (`umask 077`). Delete the report directory when it is no longer needed. Normal completion removes screenshot staging files; an abrupt process or system crash can leave a private `fp6-capture-*` directory in the temporary directory. Incomplete report directories start with `.pending-` and can be removed after checking no collection is running.

## Screenshot API assumption

The optional screenshot adapter uses the **session** bus:

- Service and interface: `org.gnome.Shell.Screenshot`
- Object: `/org/gnome/Shell/Screenshot`
- Method: `Screenshot(b include_cursor, b flash, s filename) → (b success, s filename_used)`
- Arguments: `false, false, <new private temporary directory>/screenshot.png`

This is an explicit compatibility assumption for a Lomiri shell/bridge that exposes the GNOME-style screenshot API. The locally inspected stock Lomiri source has an in-shell screenshot component, but does not establish that this D-Bus interface exists on the target port. **The phone integration must be verified.** If the service is absent or denies the call, the report records this and continues without an image; it never picks up an old screenshot or falls back to a desktop grab. Only the requested private PNG path is read, with a 32 MiB size limit; the returned path is not trusted. PNG signature checking is performed, not full image decoding.

When a screenshot is selected in the app, it minimizes itself and waits 500 ms for the shell transition before collection. The screenshot call runs first, before logs or polkit prompts. For a transient fault that disappears when switching apps, invoke the CLI capture shortcut while the affected app remains visible. See `DESIGN.md` for a quick-settings/power-menu integration proposal.

## Manual phone checklist

Use synthetic messages, SSIDs and contacts during these checks.

- [ ] Enable the module on aarch64; open **Create bug report** from the app drawer/Settings category. Confirm the Suru-style interface, scrolling, rotation, touch targets and all 12 switches.
- [ ] Confirm it starts under `systemctl --user status fp6-bugreport`, with no app or collection at login. Confirm the session environment and Maliit behavior remain normal.
- [ ] Disable all but `system`; create a report and verify only that section and the summary exist. Disable every section and confirm a clear validation error.
- [ ] Compare `15m`, `1h`, `24h` windows against synthetic journal events. Confirm failed-unit, GPU, app-ready, OOM, zram, PipeWire, thermal, storage and oFono state are useful on this port.
- [ ] Check file and directory permissions, including under a permissive shell umask. Create two reports quickly and verify neither is overwritten.
- [ ] Inject synthetic sensitive strings into a test user service's journal; verify no phone numbers, identifiers, SSIDs, IP/MAC addresses, emails, coordinates or credentials survive in extracted text. Review false positives.
- [ ] With restricted logs disabled, confirm no authorization prompt. Enable them: deny/cancel once, then approve; both paths must finish, with availability accurately summarized. Confirm an inactive session is denied and helper arguments cannot select files or commands.
- [ ] Confirm ordinary collection contains no image. In capture mode explicitly select a screenshot and verify minimization, the assumed D-Bus interface, correct affected-app pixels, and absence of a polkit dialog in the image. Repeat with the service unavailable and confirm text-only success.
- [ ] Disconnect the modem/network and stop optional services; verify a partial report rather than a crash. No network connection is needed to collect.
- [ ] Exercise missing tools, low disk space, cancellation/termination, large journals, and repeated launches. Check no complete report is announced after a write failure; remove any private crash leftovers.
- [ ] Review the archive and summary locally, confirm there are no core files or telemetry, then share a deliberately chosen test report manually.

## Development history

This workspace's `.git` is read-only. Small-step commits are delivered as `patches/*.patch`, ordered by `patches/series`; apply them to a writable checkout of the task-brief commit with `git am /path/to/patches/*.patch`. `scripts/checkpoint.py` documents the alternate-history workflow used here. `FINAL_REPORT.md` records actual verification and remaining phone-only checks.
