# fairphone6-nixos

**NixOS on the Fairphone (Gen. 6)** (SM7635 / "milos"), built with
[Mobile NixOS](https://github.com/mobile-nixos/mobile-nixos), running the
**Lomiri** shell on **Mir** (GPU, Mesa/freedreno A810) with the Ubuntu Touch
app set (dialer, messaging, contacts, camera, settings, Morph Browser,
weather, ...) packaged from nixpkgs and patched where needed.

> This is **not Ubuntu Touch** and does not use the UBports image, Halium or
> libhybris. It is plain NixOS on a mainline-based Linux kernel; only the
> Lomiri/UBports user-space components are shared with Ubuntu Touch.
>
> **Unofficial community project. Not affiliated with, endorsed by or
> supported by Fairphone B.V., UBports or the Mobile NixOS project.** Running
> this can make your phone unbootable until you restore it; see
> [Boot safety](#boot-safety) before you try anything.

The kernel is Catcrafts'
[milos-linux](https://forgejo.catcrafts.net/Catcrafts/milos-linux) (7.2.0,
the tree postmarketOS uses for the FP6) plus the patches in
`kernel-build/`. Much of the hardware enablement (device tree, drivers,
firmware packaging, libcamera tuning, UCM) comes from the postmarketOS FP6
port; this repository is the NixOS/Lomiri integration on top of it.

## Status (2026-10-26)

Tested on one FP6 (8 GB). "Live" means verified on the phone; "staged" means
built and reviewed but not yet verified on the phone.

| Area | Feature | Status |
|---|---|---|
| Boot | Lomiri + Mir on the GPU, greeter, session | works (live) |
| Boot | RAM boot (`fastboot boot`), persistent boot from boot_b after verification | works (live) |
| Boot | Quiet boot splash (Plymouth), text-free greeter hand-over | staged (`boot-splash/`) |
| Display | Scale-3 output, Qt/UITK apps, Morph Browser, Firefox, GTK3/GTK4 apps | works (live), fixes in `lomiri/`, `osk-wayland/` |
| Input | Touch, on-screen keyboard (Maliit) for Qt5, Qt6 and GTK3 Wayland clients | works (live) |
| Input | Landscape keyboard sizing, Log Out entry | staged (`lomiri-polish/`) |
| Input | Double-tap to wake (ESWIN touch lp_mode, kernel patch) | works (live, kernel5+) |
| Sensors | Proximity, ambient light, auto-brightness (STK36C61, kernel patches) | works (live) |
| Sensors | Rotation (accelerometer polling) | works (live) |
| Power | Battery percentage and capacity in µAh (qcom_battmgr patch) | works (live) |
| Power | Power key screen off/on and lock, shutdown/reboot from UI | works (live) |
| Power | RTC time without network (offset restore) | works (live) |
| Power | System suspend | **guarded off**; UFS resume fix in kernel6, supervised test pending |
| Power | Offline (charger-mode) charging | staged (`misc/charger-mode-fp6.nix`) |
| Audio | Loudspeaker, volume keys, bottom mic | works (live) |
| Audio | Top mic (SoundWire channel mapping via UCM) | fixed, enabled in the kernel6 target |
| Audio | Earpiece routing ("Voice Call" verb) | works (live) |
| Camera | Main, ultra-wide and front sensors, photo, video, flash, switching in Lomiri Camera | works (live), image tuning ongoing |
| Cellular | Modem, eSIM (lpac over QRTR), LTE registration, mobile data | works (live) |
| Cellular | SMS (oFono/telepathy), notifications | plumbing works; notification fix staged |
| Cellular | Voice calls with audio, VoLTE | **not working** (see limitations) |
| Location | Modem GNSS -> NMEA -> GeoClue, Lomiri location toggle | satellites tracked, no fix yet (indoor test only) |
| Network | Wi-Fi, Bluetooth, flight mode | works (Bluetooth pairing via Settings untested) |
| Network | USB networking (NCM), ADB | works (live) |
| Network | NetBird VPN in the network indicator | in progress |
| Other | Vibration (hfd-service, stronger magnitude) | works (live) |
| Other | Hardware video decode (Iris) incl. Lomiri media player | works (live) |
| Other | Waydroid (Android apps) with the A810 Mesa build | works, display-size fixes staged |
| Other | Fingerprint | staged; enrolment storage not enabled yet |
| Other | NFC | hardware detected, no userspace stack |
| Other | SD card automount | staged, untested (no card) |
| Apps | Pure Maps (UUITK flavour), Fragments, Lomiri Weather, Firefox | built; see `apps/`, `lomiri/` |

### Known limitations

- **No call audio and no VoLTE.** Circuit-switched voice needs a modem to
  ADSP voice path that mainline AudioReach does not have yet; IMS/VoLTE
  (imsd) does not register. SMS over SGs and LTE data work.
- **No wired external display.** The FP6 has a USB 2.0-only USB-C port (no
  DisplayPort alt mode in hardware).
- **No 3.5 mm jack**; analog USB-C headsets are not supported by the board.
  USB audio devices should work.
- **Suspend is disabled** until the UFS resume fix passes a supervised test.
- The base system still needs device-derived binary inputs (firmware, the
  original kernel tarball, a boot image template); see
  [`nixos-fp6/assets/README.md`](nixos-fp6/assets/README.md).
- Single-device testing, research-grade code: many modules carry assertions
  and comments from the bring-up and are not yet cleaned up for general use.

## Layout

| Path | Contents |
|---|---|
| `nixos-fp6/device/` | Mobile NixOS device definition (`fairphone-fp6`) |
| `nixos-fp6/default.nix`, `configuration.nix`, `*-fp6.nix` | base system and the original feature modules (audio, camera, telephony, power, Waydroid, ...) |
| `nixos-fp6/feature-push/` | current feature modules, grouped by area, and the build targets |
| `nixos-fp6/feature-push/trial-lomiri-full-kernel6-fp6.nix` | **the current full target** (Lomiri GPU, all features, kernel6) |
| `nixos-fp6/*.patch`, `feature-push/*/*.patch` | patches to user-space packages (see CONTRIBUTING-UPSTREAM.md) |
| `kernel-build/` | kernel recipes (`feature-push/kernel5`, `kernel6`), configs, kernel patches |
| `tools/` | phone helpers (RAM boot, SSH, screenshots, input), feature test scripts in `tools/feature-push/` |
| `tools/publish/` | export, secret scan and commit helpers for this repo (see PUBLISHING.md) |

## Building

Requirements: Nix (no flakes needed; plain `nix-instantiate` / `nix build
--file`) and an **aarch64-linux** builder. Native ARM is strongly
recommended: the kernel, Qt, Mesa and Firefox builds take hours.

1. Fetch the pinned sources (Mobile NixOS at
   `2c132754323fc1915e8d21dcfc0ef68ab084c6fb` plus our two patches, rmtfs,
   and a reproducible archive of milos-linux `63289f44`):

   ```sh
   tools/fetch-sources.sh            # or: --kernel-from /path/to/milos-linux checkout
   ```

   nixpkgs is pinned by Mobile NixOS' `npins` (nixos-unstable
   `26.11pre1031299.0bb7ec54c848`); nothing else is fetched outside Nix.
2. Provide the device assets in `nixos-fp6/assets/`
   ([how](nixos-fp6/assets/README.md)).
3. Evaluate (works on any Linux host):

   ```sh
   cd nixos-fp6
   nix-instantiate feature-push/trial-lomiri-full-kernel6-fp6.nix \
     -A config.system.build.toplevel -A outputs.android-bootimg
   ```
4. Build on the ARM builder, for example through `tools/nix-remote`
   (settings in `local/builder.env`, see [`local/README.md`](local/README.md)):

   ```sh
   tools/nix-remote build --eval-store auto \
     --file nixos-fp6/feature-push/trial-lomiri-full-kernel6-fp6.nix \
     config.system.build.toplevel outputs.android-bootimg --no-link --print-out-paths
   ```

The system is installed as a separate ext4 image (`/nixos-fp6.img`) on the
existing userdata root (see `device/nested-root.rb`); the kernel6 target
expects that layout.

## Boot safety

These rules come from bring-up incidents. Follow them in this order.

1. **Never `fastboot flash`** anything while testing, and never change the
   active slot (`fastboot set_active`). Keep the known-good boot image of the
   other slot untouched.
2. **RAM-boot first:** `fastboot boot boot.img` (or `tools/fastboot-ram.py`,
   which only issues the RAM-boot commands). A RAM boot leaves the flash
   unchanged; a long press on power returns to the installed system.
3. Verify the RAM-booted system completely: UI comes up, USB networking and
   SSH work (`tools/phone-ssh`), the generation is the one you built, no UFS
   or kernel errors in the journal.
4. **Only then write boot_b** (the slot used for this system), from the
   running, verified system, and read it back to compare hashes before the
   first cold boot from flash.
5. Keep a recovery path: the stock/pmOS boot image backed up off the phone,
   and a known-good RAM-bootable image.
6. Leave suspend guarded (`services.fp6SuspendPolicy`) unless you are testing
   the UFS fix under supervision: failed UFS resumes match the signature of
   an earlier root file system loss.

## Contributing upstream

Every patch we carry is inventoried in
[CONTRIBUTING-UPSTREAM.md](CONTRIBUTING-UPSTREAM.md) with its upstream,
type and status. Branches and mail-ready kernel series are prepared outside
this repository.

## Credits

- [Mobile NixOS](https://github.com/mobile-nixos/mobile-nixos) and
  [nixpkgs](https://github.com/NixOS/nixpkgs) (including the Lomiri packaging
  by the nixpkgs Lomiri maintainers).
- Catcrafts and the postmarketOS FP6 port: milos-linux kernel, device tree,
  firmware packaging, imsd, fingerprintd, the libcamera patch set in
  `nixos-fp6/camera-fp6-pmos/` (patches by their respective authors, see the
  patch headers).
- [UBports](https://ubports.com) / Lomiri, [Mir](https://github.com/canonical/mir),
  [Maliit](https://github.com/maliit), oFono, libcamera, Mesa, Waydroid.
- postmarketOS [mobile-config-firefox](https://gitlab.postmarketos.org/postmarketOS/mobile-config-firefox)
  (vendored in `nixos-fp6/firefox-mobile-config/`, MPL-2.0).

## License

Our own Nix modules, scripts and documentation are under the **MIT license**
([LICENSE](LICENSE)), matching Mobile NixOS. This is a default the maintainer
may still change.

Patches keep the license of the project they patch (for example GPL-2.0 for
kernel patches, LGPL for Qt/Lomiri/libcamera patches, MPL-2.0 for Firefox).
Files with an SPDX header (e.g. the out-of-tree S5KKD1 kernel driver,
GPL-2.0-only; the GStreamer `fp6camera` element, LGPL-2.1-or-later) are
under the license stated there. Vendored third-party trees keep their own
license files. Firmware and other device assets are not part of this
repository.
