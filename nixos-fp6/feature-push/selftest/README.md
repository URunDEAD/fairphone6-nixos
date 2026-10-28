# fp6-selftest

Private, read-only post-update health checks for Fairphone 6 / Mobile NixOS and Lomiri. The only persistent writes are private reports in `~/Documents/Self-tests/` (directory 0700, JSON and text files 0600). There is no telemetry, remote connection, audio playback, capture, discovery, service activation or settings change.

```sh
fp6-selftest                    # full checks; text on stdout; saves text + JSON
fp6-selftest --quick --json     # OTA-friendly; omits schema/launcher scans
fp6-selftest --no-save          # observation only, stdout report
fp6-selftest --config ./probes.toml --timeout 5
fp6-selftest --system-only --quick --json
```

Exit codes: **0** PASS/SKIP, **1** WARN, **2** FAIL (including timeout/configuration/save errors). SKIP is explicit, never a claim of health. WARN means a problem or incomplete coverage: disabled/offline radios, inaccessible logs, restarting services, services a target wants that are stopped, and missing audited probes. Stopped on-demand or D-Bus-activated services are PASS ("stopped until needed"). A failed installed service/probe, required shell service, dangerous schema or broken touch launcher is FAIL; the desktops check names each failing entry by its desktop id and reason. Touch entries Lomiri never lists (NoDisplay, OnlyShowIn/NotShowIn) need a valid Exec but no icon; Hidden=true entries are ignored. No failed test is repaired automatically.

The full report checks release metadata, failed system/user units, boot coredump journal records, journal errors grouped by known service, disk space, available memory, zram, display manager, Lomiri, Maliit, PipeWire/WirePlumber default sink/source, NetworkManager, Bluetooth adapter, oFono/SIM/registration, enabled mobile data contexts, GeoClue presence and fprint daemon devices. Feature probes and their validated reply shapes are in `data/probes.toml`. CLI and UI use `/etc/fp6-selftest/probes.toml` when installed by the module; an explicit `--config` takes precedence. The schema scan strictly dry-runs `glib-compile-schemas` and rejects `ai`/`ax`. Desktop checks resolve executables (including common `env` wrappers) and icons, without launching applications.

Use **Settings → Run self-test** or **Self-test** in the app drawer. The Settings page launches the Rust-backed QML runner through a registered local URI. Results remain scrollable; the UI stays responsive while checks run. A full run has 39 checks with the supplied configuration. Each has its own 5-second deadline, so a completely unresponsive system can take about 195 seconds. Quick mode still reports all base and feature probes, with explicit SKIPs for the two file scans.

## Build and tests

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell --run 'cargo test'
nix-shell --run 'cargo test --all-features'
nix-shell --run 'cargo clippy --all-targets --all-features -- -D warnings'
nix-build --no-out-link
nix-instantiate --eval --strict --json tests/module.nix --argstr system x86_64-linux
nix-instantiate --eval --strict --json tests/module.nix --argstr system aarch64-linux
```

If the channel URL cannot refresh in a restricted environment, keep `NIX_PATH` exported and pass `-I nixpkgs=/path/to/an/installed/nixpkgs` to the Nix commands. Record that checkout in build evidence.

`cargo test` needs Rust stable, `dbus-daemon`, `glib-compile-schemas`, `sh`, `head`, `sleep` and `touch` on PATH. The UI feature additionally requires Qt 5 development libraries. `shell.nix` supplies all of these and the Qt **5** build of Lomiri UI Toolkit 1.3. Build/crate caches go to `/tmp/fp6-selftest-{cargo,target}`, never into the deliverable. Nix source filtering excludes caches, previews and patches. Tests use a private D-Bus and synthetic fixtures; they do not inspect the laptop's session or phone.

```sh
nix-shell --run 'scripts/preview.sh'
```

Preview mode never starts checks. UITK shapes require OpenGL; the script uses Mesa and Xvfb while retaining `QT_QPA_PLATFORM=offscreen`. Do not set `QT_QUICK_BACKEND=software` for this toolkit. The second screenshot uses `tests/fixtures/preview.json`.

## NixOS integration

Add this module relative to your configuration:

```nix
imports = [ ./selftest/module.nix ];
hardware.fairphone6.selftest = {
  enable = true;
  user = "phablet"; # existing normal user
  # probeFile = ./phone-selftest-probes.toml;
  # sessionTarget = "lomiri-session.target"; # if used by this phone image
  # automatic = false; # manual-only deployment
};
```

The module installs a Qt 5 UI, X-Lomiri-Touch desktop file, Settings plugin, URI association, system/user oneshots and boot/session timers. System checks run as the phone user two minutes after boot; session checks run 90 seconds after the graphical session starts. Both save to the user's private Documents directory. They run after OTA reboots as well as ordinary boots; no generation marker is written. WARN is a successful systemd completion; FAIL remains failed. No root privilege or journal-group membership is granted. Missing system journal access is reported as WARN.

Settings discovers the plugin via XDG data paths and `/share/lomiri-system-settings`, which the module links into the system profile. Verify discovery and URI dispatch on the phone's patched Settings package. The module does not modify the shared trial configuration or the updater. See `DESIGN.md` for the explicit OTA hook contract and remaining interface slots.

## Manual phone checklist (coordinator)

- [ ] Import module, enable for the real login user, build aarch64 and boot the image.
- [ ] Open Settings → Run self-test; verify button opens the runner and starts once. Also launch from app drawer, scroll results, run again.
- [ ] Run full and quick CLI reports; verify valid JSON, exit codes, timestamps, 0600 files and 0700 directory under Documents/Self-tests.
- [ ] Confirm boot and session timers run after OTA reboot without racing the graphical session.
- [ ] Check normal audio defaults, Maliit, display/Lomiri, modem online/SIM/registration, data context, GPS and fingerprint service adapters against the actual phone contracts.
- [ ] With airplane mode, missing SIM, disabled data and stopped optional features, verify WARN/SKIP and no state change. Observe no playback, capture, Bluetooth discovery or new network activity.
- [x] Resolve every `unverified` feature slot against real APIs (done for r5, 2026-10-28); mark mandatory installed services `required=true` when wanted.
- [ ] Give the netpolicy helper an upstream read-only status API if deeper coverage is required. Until then its owned, introspected interface passes as presence-only.
- [ ] Validate system journal visibility without escalating privileges; compare error counts with operator-visible logs. Journal reads cap at 1000 entries per check.
- [ ] Test deliberate broken schema/desktop entry in a disposable profile and a delayed mock service; confirm FAIL and bounded runtime.
- [ ] Confirm the OTA start-check consumer treats SKIP/WARN as incomplete coverage and never marks a boot healthy solely on a summary exit code.
