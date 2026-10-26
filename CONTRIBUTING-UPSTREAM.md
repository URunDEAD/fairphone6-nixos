# Contributing our fixes upstream

Inventory of every patch and override this port carries, where it belongs
upstream, and what is prepared. Paths are relative to `nixos-fp6/` unless
they start with `kernel-build/`. "Target" means the patch is used by the
current target (`feature-push/trial-lomiri-full-kernel6-fp6.nix`); "legacy"
means only older trial targets use it.

Status: **ready** = branch or patch series prepared, can be submitted after
review; **needs-work** = real improvement that needs a rebase, a design
discussion or more testing; **local-only** = FP6/NixOS port hack, a
third-party patch we only carry, or already fixed upstream.

Prepared branches and series live outside this repository, in a sibling
directory `fp6-upstream/<project>/` (one clone per upstream project; branch
names `fp6/<topic>`). Nothing has been pushed or sent yet.

Summary:

| Upstream | ready | needs-work | where |
|---|---|---|---|
| Linux mainline (UFS, qcom_battmgr, stk3310) | 2 (+1 RFC) | qcomtee CMA | `fp6-upstream/linux/series/` (git format-patch, mail) |
| milos-linux (Catcrafts) | CAMSS fixes (3), ESWIN gesture wake | - | `fp6-upstream/linux/series/` (mail or Forgejo PR) |
| oFono | provision.db path (2 patches) | - | `fp6-upstream/ofono/series/` (mail) |
| Mobile NixOS (GitHub) | zstd initrd, ffs module name, gt CMake 4 | qrtr/tqftpserv from nixpkgs, FP6 device | branches in `fp6-upstream/mobile-nixos` |
| nixpkgs (GitHub) | ofono, lpac QRTR, pure-maps, tqftpserv | lomiri-system-settings battery panel, telephony plugin paths, new Lomiri telephony packages | branches in `fp6-upstream/nixpkgs` |
| UBports/Lomiri (GitLab) | qtmir (3), camera app, telephony-service (3), hfd, morph-browser, system-settings, UITK, nuntium (2), qtcontacts-sqlite | qtmir (5, need rebase), Lomiri scale/OSK/session patches | branches in `fp6-upstream/<project>` |
| maliit-framework (GitHub) | 2 | maliit-keyboard panel size hint | branches in `fp6-upstream/maliit-framework` |
| Others | imsd, vvmplayer, Chatty test | Firefox, Mesa, Qt 5 backports, libcamera helper | see tables |

## Linux kernel and oFono

Mail-ready series: `fp6-upstream/linux/series/<topic>/` (cover letter,
`to-cc.txt` from `scripts/get_maintainer.pl`, checkpatch results in
`fp6-upstream/linux/README.md`) and `fp6-upstream/ofono/series/`.
The kernel's `Documentation/process/coding-assistants.rst` says AI tools
must not add `Signed-off-by`; the commits carry an `Assisted-by:` tag
instead, and `fp6-upstream/linux/make-series.sh` with `SIGNOFF=1` adds the
maintainer's own `Signed-off-by` (which certifies the
[DCO](https://developercertificate.org)) only after he has reviewed the
patches. oFono does not use Signed-off-by at all.

| patch | upstream | URL | type | status | notes |
|---|---|---|---|---|---|
| kernel-build/feature-push/suspend/0001-scsi-ufs-qcom-milos-power-down-link-in-system-suspend.patch | Linux (SCSI/UFS, ufs-qcom) | https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git | bugfix | needs-work (RFC prepared) | Applies to mainline 6b4b543fa187. Series `fp6-upstream/linux/series/ufs-qcom-milos` (RFC). Hold non-RFC until the supervised suspend/resume test passes. |
| kernel-build/feature-push/misc/0001-power-supply-qcom_battmgr-report-charge-in-uAh-on-SM8350-family.patch | Linux (power/supply, qcom_battmgr) | https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git | bugfix | ready | Live-verified on FP6 (charge_full 4517000 uAh). Affects all SM8350/SM8550-variant devices. Series `series/qcom-battmgr-charge-uah`. |
| kernel-build/feature-push/sensors/0001-iio-light-stk3310-noirq-polling-event-free.patch | Linux (IIO, stk3310) | https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git | bugfix | ready | Applies to mainline. Series `series/stk3310-no-irq`. |
| kernel-build/feature-push/sensors/0002-iio-light-stk3310-retry-transient-bus-errors.patch | Linux (IIO, stk3310) | https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git | hack | local-only | Works around FP6 i2c-gpio NAKs; retries in drivers are usually rejected. |
| kernel-build/feature-push/sensors/0003-input-eswin-eph861x-gesture-wake-pm-lp-mode.patch | milos-linux (Catcrafts), driver not in mainline | https://forgejo.catcrafts.net/Catcrafts/milos-linux | feature | ready (for milos-linux) | Live: double-tap wake works. Vendor-style indentation kept. Series `series/milos-eswin-gesture-wake`; forgejo PR or mail to the tree owner. |
| kernel-build/selfie-dphy/milos-dphy-no-cphy-rate-table.patch | milos-linux (CAMSS Milos) | https://forgejo.catcrafts.net/Catcrafts/milos-linux | bugfix | ready (for milos-linux) | Series `series/milos-camss-fixes` 1/3. Milos CAMSS not in mainline. |
| kernel-build/selfie-rdi6/csid340-phy-select-three-bits.patch (= selfie-csid5, nixos-fp6/selfie-camss-kernel4) | milos-linux (CAMSS csid-340) | https://forgejo.catcrafts.net/Catcrafts/milos-linux | bugfix | ready (for milos-linux) | Series 2/3. Mainline csid-340 only serves QCM2290 (2 PHYs), so the bug only shows with Milos. |
| kernel-build/selfie-rdi6/milos-rdi-align-16.patch (= nixos-fp6/selfie-camss-kernel4) | milos-linux (CAMSS vfe) | https://forgejo.catcrafts.net/Catcrafts/milos-linux | bugfix | ready (for milos-linux) | Series 3/3. |
| kernel-build/gmu-timeout/gmu-fw-init-timeout-100ms.patch | Linux (drm/msm) | https://lore.kernel.org/ (Rob Clark, posted 2026-09-12) | bugfix (backport) | local-only | Not ours; drop once merged. |
| kernel-build/qcomtee-cma/qcomtee-large-shm-cma.patch | Linux (tee/qcomtee) | https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git | hack | needs-work | CMA workaround for large shm; needs design discussion first. |
| kernel-build/feature-push/misc/0002-EXPERIMENTAL-arm64-dts-milos-fp6-rtc-allow-set-time.patch | milos-linux / Linux (arm64 dts) | https://forgejo.catcrafts.net/Catcrafts/milos-linux | hack | local-only | Experimental, not applied; userspace RTC offset used instead. |
| kernel-build/feature-push/network/0001-ath11k-wcn6750-experimental-p2p-modes.patch | Linux (ath11k) | https://git.kernel.org/pub/scm/linux/kernel/git/ath/ath.git | feature | local-only | Experimental, untested; firmware P2P support unknown. |
| kernel-build/feature-push/camera/0001-fp6-s5kkd1-dt-front-rotation-270.patch | ours (out-of-tree DT include) | - | feature | local-only | Not kernel source; rotation unconfirmed. |
| nixos-fp6/fp6-gap-proposals/stk36c61-binding.patch, stk3310-noirq*.patch | Linux (IIO) | - | - | local-only | Binding is Jorijn van der Graaf's (already mainline); noirq drafts superseded by 0001. |
| ofono postPatch provisiondb path (nixos-fp6/feature-push/cellular/packages.nix) | oFono | https://git.kernel.org/pub/scm/network/ofono/ofono.git | bugfix | ready | Proper fix: use $(pkgdatadir). Branch `fp6-upstream/ofono` `fp6/provisiondb-pkgdatadir`, series in `ofono/series/provisiondb-pkgdatadir` (2 patches, no SoB per oFono rules). Build-verified on 2.19. |
| ofono `--localstatedir=/var` (same file) | nixpkgs (ofono package) | https://github.com/NixOS/nixpkgs | bugfix | ready (nixpkgs fork) | Packaging only: STORAGEDIR under the read-only store. Not an oFono bug. |

## Mobile NixOS and nixpkgs

| patch | upstream | URL | type | status | notes |
|---|---|---|---|---|---|
| `mobile-nixos-zstd.patch` (also applied in the vendored `upstream/mobile-nixos/modules/initrd.nix`) | Mobile NixOS | https://github.com/mobile-nixos/mobile-nixos | feature | ready | Branch `fp6/initrd-zstd` (base 2c13275 = current master). Fork: `https://github.com/URunDEAD/mobile-nixos.git`. Target. |
| `mobile-nixos-ffs-module.patch` (also in vendored `modules/initrd-usb.nix`) | Mobile NixOS | same | bugfix | ready | `usb_f_ffs` -> `usb_f_fs`. Branch `fp6/ffs-module-name`. Target. |
| `configuration.nix`: `gadget-tool` `CMAKE_POLICY_VERSION_MINIMUM=3.5` | Mobile NixOS (`overlay/gt`) | same | bugfix | ready | gt does not configure with CMake 4. Branch `fp6/gt-cmake4`. Target. |
| `feature-push/cellular/packages.nix`: tqftpserv v1.2 + qrtr from nixpkgs (overlay shadows them with 2020 snapshots) | Mobile NixOS (`overlay/qrtr`) | same | bugfix | needs-work | Branch `fp6/qrtr-userspace-from-nixpkgs` drops the overlay's qrtr/qmic/tqftpserv/rmtfs in favour of nixpkgs. Depends on the nixpkgs tqftpserv fix below; pd-mapper (kept) is not rebuilt/tested against the new qrtr. The FP6 notes say nixpkgs' tqftpserv 1.1-unstable-2025-08-01 stored WRQ uploads as 0-byte files; upstream is at v1.3 now, retest before submitting. Target. |
| `device/` (FP6 device: default.nix, kernel.nix, nested-root.rb) | Mobile NixOS (`devices/fairphone-fp6`) | same | feature | needs-work | Draft branch `fp6/fairphone-fp6` (stacked on zstd + ffs). Kernel from Catcrafts milos-linux 63289f4 with kernel6 config, but source hash is `lib.fakeHash`; firmware and boot template via `requireFile`; boot image still repacked from a header v2 template (`repack-boot.py`). `nested-root.rb` (nested userdata root, fixed UUID) is port-specific and left out. Evaluates (`identity.name`, `system`). |
| `configuration.nix`: `rmtfs` 1.3 override | nixpkgs `rmtfs` | https://github.com/NixOS/nixpkgs | bugfix | local-only | Already fixed: nixpkgs master has rmtfs 1.3. Drop the override after the next nixpkgs bump. |
| `feature-push/cellular/packages.nix`: ofono `--localstatedir=/var` + provision.db path | nixpkgs `ofono` | https://github.com/NixOS/nixpkgs | bugfix | ready | Branch `fp6/ofono-storage-provisiondb` (base master 3e2b01f). Parses. Proper upstream oFono fix (look up provision.db in PKGDATADIR) is handled with the oFono series. Fork: `https://github.com/URunDEAD/nixpkgs.git`. Target. |
| `feature-push/cellular/lpac-qrtr.nix`: `LPAC_WITH_APDU_QMI_QRTR` | nixpkgs `lpac` | same | feature | ready | Branch `fp6/lpac-qmi-qrtr`, adds `withQmiQrtr` (+ libqrtr-glib). Verified on device: `lpac chip info` / `profile list` over QRTR. Target. |
| `feature-push/apps/maps-fp6.nix`: Pure Maps missing QtGraphicalEffects | nixpkgs `pure-maps` | same | bugfix | ready | Branch `fp6/pure-maps-qtgraphicaleffects`. The UUITK flavour and DATADIR_RUNNING substitution in our module are FP6 choices (local-only). Target. |
| nixpkgs `tqftpserv` postPatch drops the trailing slash of FIRMWARE_BASE (found while reviewing; we bypass the package) | nixpkgs `tqftpserv` | same | bugfix | ready | Branch `fp6/tqftpserv-firmware-base`. Paths became `.../uncompressed-firmwareqcom/...`. Consider also bumping to linux-msm v1.3 (untested on FP6). |
| `feature-push/settings/settings-fp6.nix`: battery panel `visible-if-file-exists com.lomiri.Repowerd.conf` | nixpkgs `lomiri-system-settings` | same | bugfix | needs-work | nixpkgs rewrites the check to `/run/current-system/sw/etc`, but repowerd is not packaged, so the Battery panel is never shown on NixOS. Needs a decision (package repowerd vs. drop the check). |
| `feature-push/settings/lss-build-about-brightness.patch` + `android-properties-shim.h` | lomiri-system-settings / nixpkgs | https://gitlab.com/ubports/development/core/lomiri-system-settings | hack | local-only | Builds About/Brightness without libhybris via a header shim. A clean version would make the ANDR_PROP dependency optional upstream. |
| `configuration.nix`: qtmir postPatch accepting `..maliit-keyboard-wrapped-wrapped` as input-method binary | nixpkgs `qtmir` / qtmir | https://gitlab.com/ubports/development/core/qtmir | hack | needs-work | qtmir checks `/proc/PID/exe` against `/usr/bin/maliit6-server`; on NixOS the wrapped binary has a store path. Needs an upstream way to configure trusted IM binaries. |
| `feature-push/cellular/messaging-fp6.nix`: QT_PLUGIN_PATH wrappers for telephony-service + telepathy-ofono | nixpkgs | https://github.com/NixOS/nixpkgs | bugfix | needs-work | Contacts plugins are not on the plugin path; needs qtcontacts-sqlite packaged in nixpkgs first. |
| `telephony-service-native-overlay.nix`: handler user unit `/usr/bin/` | nixpkgs `lomiri-telephony-service` | same | bugfix | needs-work | nixpkgs should substitute the Debian-style unit paths; check against the current nixpkgs module first. |
| `feature-push/cellular/packages.nix`: NetworkManager `-Dofono=true` | nixpkgs `networkmanager` | same | feature | local-only | Could become a `withOfono` option; low priority. |
| `feature-push/lomiri/lomiri-gpu-fp6.nix`: skip UITK `test_back_button_wide` | lomiri-ui-toolkit | https://gitlab.com/ubports/development/core/lomiri-ui-toolkit | hack | local-only | Timing-flaky test on a loaded builder. |
| New packages we wrote: `telepathy-ofono-package.nix`, `ofono-qt-package.nix`, `qtcontacts-sqlite-package.nix`, `nuntium-package.nix`, `online-accounts-bindings-package.nix`, `addressbook-frontend-package.nix`, `telephony-frontend-package.nix` (dialer/messaging), `sqlite-icu-extension-package.nix`, `feature-push/lomiri/lomiri-weather-app.nix` | nixpkgs | same | feature | needs-work | Candidates for new nixpkgs packages (Lomiri telephony stack, Weather). Each needs a nixpkgs-style package file, meta, maintainer and tests. |

## UBports / Lomiri (GitLab)

| patch | upstream | URL | type | status | notes |
|---|---|---|---|---|---|
| `qtmir-scaled-buffer.patch` | qtmir | https://gitlab.com/ubports/development/core/qtmir | bugfix | ready | Branch `fp6/scaled-display-buffer` on main 38592b24. Fork: `git@gitlab.com:<gitlab-user>/qtmir.git`. Target. |
| `qtmir-initial-focus.patch` | qtmir | same | bugfix | ready | Branch `fp6/initial-screen-focus` on main. Target. |
| `qtmir-orientation-owner-thread.patch` | qtmir | same | bugfix | ready | Branch `fp6/orientation-owner-thread` on main. Target. |
| `qtmir-initial-input-region.patch` | qtmir | same | bugfix | needs-work | Branch `fp6/initial-input-region` on 57d9e97 (version nixpkgs packages); conflicts with main (Mir 2 port), needs rebase. Target. |
| `qtmir-initial-child-placement.patch` | qtmir | same | bugfix | needs-work | Branch `fp6/initial-child-placement` (on top of input-region, base 57d9e97); needs rebase. Target. |
| `qtmir-input-method-lifecycle.patch` | qtmir | same | bugfix | needs-work | Branch `fp6/input-method-lifecycle` (57d9e97); needs rebase. Test path no longer mentions /nix/store. Target. |
| `feature-push/lomiri/qtmir-subsurface-stretch.patch` | qtmir | same | bugfix | needs-work | Branch `fp6/subsurface-stretch` (57d9e97); needs rebase. Target. |
| `feature-push/lomiri/qtmir-scaled-pad-or-crop.patch` | qtmir | same | bugfix | needs-work | Branch `fp6/pad-or-crop-logical` (57d9e97); needs rebase. Target. |
| `lomiri-input-method-scale.patch`, `lomiri-wayland-surface-scale.patch` | Lomiri | https://gitlab.com/ubports/development/core/lomiri | bugfix | needs-work | Wayland output scale 3 support in the shell (OSK and app surfaces). Belongs with the qtmir scale fixes; needs discussion of fillMode Stretch vs PadOrCrop with upstream. Target. |
| `feature-push/lomiri/lomiri-staged-maximized.patch` | Lomiri | same | feature | needs-work | Report staged windows as maximized so GTK/libadwaita CSD drops shadows and buttons. Target. |
| `feature-push/lomiri-polish/lomiri-input-panel-size-hint.patch` + `maliit-keyboard-panel-size-hint.patch` | Lomiri + maliit-keyboard | same / https://github.com/maliit/keyboard | feature | needs-work | Paired API (shell publishes input-panel size hint, keyboard uses it in landscape). Contains FP6-specific test values; needs an agreed interface. Target. |
| `feature-push/lomiri-polish/lomiri-session-entries-phone.patch`, postPatch Log Out fallback / camera cutout gap | Lomiri | same | feature | needs-work | Log Out on phones, session indicator in the expanded panel. Policy change; the cutout gap is FP6-specific (local-only). Target. |
| `lomiri-relocatable-installed-paths.patch` | Lomiri | same | hack | local-only | Keeps store paths ASCII for closure relocation in our build pipeline. |
| `lomiri-touch-diagnostics.patch` | Lomiri | same | hack | local-only | Diagnostics only. Legacy. |
| `lomiri-session-wayland-grid.patch` | lomiri-session | https://gitlab.com/ubports/development/core/lomiri-session | bugfix | needs-work | Publishes a logical GRID_UNIT_PX for scaled Wayland clients. Part of the scale series. Target. |
| `feature-push/camera/lomiri-camera-app-wayland-device-mode.patch` | lomiri-camera-app | https://gitlab.com/ubports/development/apps/lomiri-camera-app | bugfix | ready | Branch `fp6/wayland-platform-device-mode` on main b75af54 (v4.2.1). Fork: `git@gitlab.com:<gitlab-user>/lomiri-camera-app.git`. Target. |
| `lomiri-camera-app-init-mode.patch`, `lomiri-camera-app-single-qguiapplication.patch` | lomiri-camera-app | same | bugfix | local-only | Already fixed upstream in 4.2.1 (95a07e5, 2d7265d). nixpkgs should bump 4.1.1 -> 4.2.1. Target. |
| `lomiri-camera-app-slideshow-exif.patch` | lomiri-camera-app | same | bugfix | needs-work | Upstream replaced the media viewer after 4.1.1; recheck EXIF rotation on 4.2.1 first. Target. |
| `lomiri-camera-app-front-viewfinder.patch` | lomiri-camera-app | same | bugfix | needs-work | Assumes a libcamera source that does not mirror front frames; needs a generic way to tell. Target. |
| `feature-push/cellular/telephony-service-contacts-fallback.patch` | lomiri-telephony-service | https://gitlab.com/ubports/development/core/lomiri-telephony-service | bugfix | ready | Upstream main already switched to qtcontacts-sqlite + `LOMIRI_CONTACT_BACKEND`; the remaining bug (notification dropped when `request->start()` fails) is on branch `fp6/notify-without-contacts`. Fork: `git@gitlab.com:<gitlab-user>/lomiri-telephony-service.git`. Target. |
| `feature-push/audio/telephony-service-ucm-port-names.patch` | lomiri-telephony-service | same | feature | ready | UCM port names / split Voice Call profiles for PipeWire. Branch `fp6/ucm-port-names` on main. Target. |
| `telephony-service-native-overlay.nix`: indicator D-Bus service `Exec` name | lomiri-telephony-service | same | bugfix | ready | `.service` file still says `lomiri-telephony-service-indicator`. Branch `fp6/indicator-dbus-exec`. |
| `feature-push/misc/hfd-ff-magnitude.patch` | hfd-service | https://gitlab.com/ubports/development/core/hfd-service | feature | ready | `HFD_FF_MAGNITUDE` env; debug print removed. Branch `fp6/ff-magnitude-env`. Fork: `git@gitlab.com:<gitlab-user>/hfd-service.git`. Target. |
| `feature-push/osk-wayland/morph-browser-defer-user-agent-change.patch` | morph-browser | https://gitlab.com/ubports/development/core/morph-browser | bugfix | ready | Browser.qml half fixed upstream (8bfb48a6, 1.99.5); webcontainer half on branch `fp6/webcontainer-defer-user-agent`. nixpkgs should bump 1.99.4. Target. |
| `feature-push/settings/lss-brightness-define-adapter-slot.patch` | lomiri-system-settings | https://gitlab.com/ubports/development/core/lomiri-system-settings | bugfix | ready | Branch `fp6/define-adapter-slot` on main. Target. |
| `nuntium-optional-preferred.patch` | nuntium | https://gitlab.com/ubports/development/core/nuntium | bugfix | ready | Mainline oFono has no "Preferred" property (panic). Branch `fp6/optional-preferred`. Target. |
| `nuntium-go-ldm-update.patch` | nuntium | same | bugfix | ready | Branch `fp6/update-go-ldm`. Target. |
| `qtcontacts-sqlite-qt515-test.patch` | qtcontacts-sqlite (UBports fork; origin sailfishos) | https://gitlab.com/ubports/development/core/qtcontacts-sqlite | bugfix | ready | Branch `fp6/qt515-datetime-test`. Also relevant to https://github.com/sailfishos/qtcontacts-sqlite. Target. |
| `uitk-async-loader-lifetime.patch` | lomiri-ui-toolkit | https://gitlab.com/ubports/development/core/lomiri-ui-toolkit | bugfix | ready | Use-after-free when a loader is destroyed while compiling. Branch `fp6/async-loader-lifetime` on main. Legacy (not in current target). |
| `stevia-fp6-compact-landscape.patch` | Stevia (Phosh OSK) | https://gitlab.gnome.org/World/Phosh/stevia | feature | local-only | Legacy Phosh target. |

## Other projects

| patch | upstream | URL | type | status | notes |
|---|---|---|---|---|---|
| `maliit-standalone-input-area.patch` | maliit-framework | https://github.com/maliit/framework | bugfix | ready | Branch `fp6/standalone-input-method-area` on master ba6f7ed. Fork: `https://github.com/URunDEAD/maliit-framework.git`. Target. |
| `feature-push/osk-wayland/maliit-hybrid-dbus-wayland-connection.patch` | maliit-framework | same | feature | ready | Opt-in `MALIIT_HYBRID_CONNECTION=1`; FP6 comments removed. Branch `fp6/hybrid-dbus-wayland-connection`. Target. |
| `mir-input-method-v1.patch` | Mir | https://github.com/canonical/mir | feature | local-only | Backport of upstream PR #3000 (merged as 99644e44) to Mir 2.15. Nothing to send. Target. |
| `upstream/mir`, `upstream/maliit-*`, `upstream/rmtfs`, `upstream/deviceinfo` checkouts | various | - | - | local-only | No local modifications (clean at upstream commits). `upstream/qtmir` has uncommitted edits equal to `qtmir-initial-focus.patch` + `qtmir-scaled-buffer.patch` (covered above). |
| `feature-push/audio/imsd-media-role-phone.patch` | imsd (Catcrafts) | https://forgejo.catcrafts.net/Catcrafts/imsd | bugfix | ready | Branch `fp6/media-role-phone` on 2b5de7a. Fork needs a Forgejo account on forgejo.catcrafts.net. Target. |
| `postmarketos-apps-vvmplayer-eds360.patch` | vvmplayer | https://gitlab.com/kop316/vvmplayer | bugfix | ready | EDS 3.60 build fix. Branch `fp6/eds-3.60`. May need an EDS version guard if upstream supports older EDS. Target (pmOS app set). |
| `chatty-history-test-finalize.patch` | Chatty | https://gitlab.gnome.org/World/Chatty | bugfix | ready | Flaky test. Branch `fp6/history-test-async-finalize` (shallow clone of main). Needs a GNOME GitLab account. |
| `chatty-optional-matrix.patch` | Chatty | same | feature | needs-work | Build without libcmatrix; does not apply to main any more. |
| `feature-push/lomiri/firefox-wayland-buffer-scale-fallback.patch` | Firefox | https://bugzilla.mozilla.org (Phabricator) | bugfix | needs-work | Integer buffer-scale fallback when the compositor has no wp_viewporter (Mir 2.15). Needs a Bugzilla bug + Phabricator revision. Target. |
| `mesa-aarch64-os-features.patch` | Mesa | https://gitlab.freedesktop.org/mesa/mesa | bugfix | needs-work | `DETECT_ARCH_AARCH64` in `lp_build_fill_mattrs` is already in main; the "generic CPU model on aarch64 (no OS-disabled SVE)" part still needs checking against main and an MR. Target. |
| `camera-fp6-pipewire-metadata-listener.patch` | PipeWire | https://gitlab.freedesktop.org/pipewire/pipewire | bugfix | local-only | Backport of an upstream commit (Julian Bouzas). Target. |
| `feature-push/camera/qtmultimedia-camerabin-gstinit-fp6.patch` | Qt 5 qtmultimedia | https://invent.kde.org/qt/qt/qtmultimedia (kde/5.15) | bugfix | needs-work | camerabin uses `m_sourceFactory` before init (ServiceMissing). Qt 5 is EOL; KDE's collection takes backports only. Target. |
| `feature-push/camera/qtmultimedia-camerabin-fp6.patch` | Qt 5 qtmultimedia | same | hack | local-only | FILL_IN = torch mapping and x264enc tuning for the phone CPU. Target. |
| `qt5-borderimage/stale-reply.patch` | Qt 5 qtdeclarative | https://invent.kde.org/qt/qt/qtdeclarative | bugfix | needs-work | Stale network reply crash in BorderImage; check Qt 6 first (upstream fix must land there). Target. |
| `feature-push/lomiri-polish/qtsensors-iio-reclaim/iio-sensor-proxy-reclaim.patch` | Qt 5 qtsensors (iio-sensor-proxy backend) | https://invent.kde.org/qt/qt/qtsensors | bugfix | needs-work | Re-claim after proxy restart; built out of tree as a separate plugin. Qt 5 EOL. Target. |
| `selfie-fp6-libcamera-helper.patch`, `camera-fp6-sensor-test.patch` | libcamera | https://git.libcamera.org/libcamera/libcamera.git (mailing list) | feature | needs-work | Gain helper for our out-of-tree S5KKD1 driver name `fp6-s5kkd1`; needs the mainline sensor driver first. Target. |
| `camera-fp6-pmos/0001-0016` | libcamera (postmarketOS / Catcrafts patch set) | https://gitlab.postmarketos.org/postmarketOS/pmaports | feature | local-only | Third-party patches (Collabora, Catcrafts, Vasiliy Doylov, Pavel Machek). Not ours to send. Target. |
| `camera-fp6-sensor-helpers.patch` | libcamera | as above | feature | local-only | Superseded. Legacy. |
| `fingerprintd-*.patch` (enrollment preservation, transport diagnostics; others legacy) | fingerprintd (Catcrafts) | https://forgejo.catcrafts.net/Catcrafts/fingerprintd | feature | local-only | Diagnostics/metrics and FP6 storage policy. Not prepared. |
| `feature-push/fingerprint/biometryd-fprintd/`, `fingerprint-id-app/` | biometryd / new | https://gitlab.com/ubports/development/core/biometryd | feature | needs-work | Our own bridge and app; would need to be proposed as new components. |
| `geoclue-standalone.patch` | GeoClue | https://gitlab.freedesktop.org/geoclue/geoclue | hack | local-only | Legacy. |
| `ffmpeg-fp6-*.patch` | FFmpeg | https://ffmpeg.org (ffmpeg-devel) | bugfix | local-only | Legacy V4L2 M2M experiments (EOS drain, NV12 padding, diagnostics); not validated for upstream. |
| `camera-v4l2loopback-mjpg16.patch` | v4l2loopback | https://github.com/umlaeute/v4l2loopback | hack | local-only | Legacy. |
| `camera-android-*` (incl. `waydroid-abi/*` libhidl/libhwbinder patches, minigbm yv12, provider path length) | Waydroid / AOSP | https://github.com/waydroid | hack | local-only | Copies of Waydroid patches plus local Android camera-provider build hacks. Legacy. |

## Branches prepared

All commits: author and committer Paul Schuldesz, ending with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. GitHub forks exist under URunDEAD for mobile-nixos, nixpkgs, maliit-framework,
maliit-keyboard, tqftpserv and mir and are configured as remote `fork` (not pushed yet).
GitLab/Forgejo/GNOME forks still have to be created.

| repo (`fp6-upstream/...`) | branch | base | commit | fork remote to add |
|---|---|---|---|---|
| mobile-nixos | fp6/initrd-zstd | 2c13275 (master) | d405db1b | https://github.com/URunDEAD/mobile-nixos.git |
| mobile-nixos | fp6/ffs-module-name | 2c13275 | 83dcf6d5 | same |
| mobile-nixos | fp6/gt-cmake4 | 2c13275 | 144c3a43 | same |
| mobile-nixos | fp6/qrtr-userspace-from-nixpkgs (needs-work) | 2c13275 | 7455c4f3 | same |
| mobile-nixos | fp6/fairphone-fp6 (draft; on zstd + ffs) | 2c13275 | e192f725 | same |
| nixpkgs (sparse, depth 1) | fp6/ofono-storage-provisiondb | 3e2b01f (master) | 851851d9 | https://github.com/URunDEAD/nixpkgs.git |
| nixpkgs | fp6/lpac-qmi-qrtr | 3e2b01f | 9cc98da8 | same |
| nixpkgs | fp6/pure-maps-qtgraphicaleffects | 3e2b01f | 179e3c09 | same |
| nixpkgs | fp6/tqftpserv-firmware-base | 3e2b01f | 38b77c24 | same |
| qtmir | fp6/scaled-display-buffer | 38592b24 (main) | c0edc3c0 | git@gitlab.com:<gitlab-user>/qtmir.git |
| qtmir | fp6/initial-screen-focus | 38592b24 | 4c090c3a | same |
| qtmir | fp6/orientation-owner-thread | 38592b24 | c589be3e | same |
| qtmir | fp6/initial-input-region (needs rebase) | 57d9e97 | 283b5d9a | same |
| qtmir | fp6/initial-child-placement (needs rebase; on input-region) | 57d9e97 | 0c171b87 | same |
| qtmir | fp6/input-method-lifecycle (needs rebase) | 57d9e97 | 3e3264c8 | same |
| qtmir | fp6/subsurface-stretch (needs rebase) | 57d9e97 | 9aece98d | same |
| qtmir | fp6/pad-or-crop-logical (needs rebase) | 57d9e97 | 93b7dd9f | same |
| lomiri-camera-app | fp6/wayland-platform-device-mode | b75af54 (main) | facbda4 | git@gitlab.com:<gitlab-user>/lomiri-camera-app.git |
| lomiri-telephony-service | fp6/notify-without-contacts | 8b33e68d (main) | 9191cd3d | git@gitlab.com:<gitlab-user>/lomiri-telephony-service.git |
| lomiri-telephony-service | fp6/ucm-port-names | 8b33e68d | 4d51ab42 | same |
| lomiri-telephony-service | fp6/indicator-dbus-exec | 8b33e68d | e14bd7fc | same |
| hfd-service | fp6/ff-magnitude-env | 87f4582 (main) | caa82c5 | git@gitlab.com:<gitlab-user>/hfd-service.git |
| morph-browser | fp6/webcontainer-defer-user-agent | dcd75cb9 (main) | be6ae57e | git@gitlab.com:<gitlab-user>/morph-browser.git |
| lomiri-system-settings | fp6/define-adapter-slot | f183e28a (main) | 6f608c0b | git@gitlab.com:<gitlab-user>/lomiri-system-settings.git |
| lomiri-ui-toolkit (depth 30) | fp6/async-loader-lifetime | add3352 (main) | 0b966f6 | git@gitlab.com:<gitlab-user>/lomiri-ui-toolkit.git |
| nuntium | fp6/optional-preferred | d051757 (main) | 248453d | git@gitlab.com:<gitlab-user>/nuntium.git |
| nuntium | fp6/update-go-ldm | d051757 | de3d40a | same |
| qtcontacts-sqlite | fp6/qt515-datetime-test | ea00c0b (main) | f70a399 | git@gitlab.com:<gitlab-user>/qtcontacts-sqlite.git |
| maliit-framework | fp6/standalone-input-method-area | ba6f7ed (master) | 945b6fa | https://github.com/URunDEAD/maliit-framework.git |
| maliit-framework | fp6/hybrid-dbus-wayland-connection | ba6f7ed | 9924dea | same |
| imsd | fp6/media-role-phone | 2b5de7a | 270c99e | Forgejo fork on forgejo.catcrafts.net |
| vvmplayer | fp6/eds-3.60 | be69e29 (2.6) | c1a3056 | git@gitlab.com:<gitlab-user>/vvmplayer.git |
| chatty (depth 50) | fp6/history-test-async-finalize | 3941351 (main) | 97a8d78 | git@gitlab.gnome.org:<gnome-gitlab-user>/Chatty.git |

Checks: `nix-instantiate --parse` on every touched .nix file (OK); the FP6 device draft evaluates
`mobile.device.identity.name` and `mobile.system.system`. No C/C++/QML build was run for the
GitLab/GitHub branches; every patch except the cleaned-up comments is the one built and tested in
the FP6 tree (rebased hunks: telephony-service notify, morph-browser webcontainer, vvmplayer).
| linux/mainline (sparse, depth 1) | fp6/ufs-qcom-milos-spm-lvl5 (RFC) | torvalds 6b4b543fa187 | e644d9ef03ae | mail: `series/ufs-qcom-milos` |
| linux/mainline | fp6/qcom-battmgr-charge-uah | 6b4b543fa187 | b1eb0d979d01 | mail: `series/qcom-battmgr-charge-uah` |
| linux/mainline | fp6/stk3310-no-irq | 6b4b543fa187 | 885c7af1119f | mail: `series/stk3310-no-irq` |
| linux/milos | fp6/camss-milos-fixes (3 patches) | milos-linux 63289f441152 | 293ae1d698ea | mail or Forgejo PR: `series/milos-camss-fixes` |
| linux/milos | fp6/eswin-gesture-wake | 63289f441152 | 96319fe158cd | mail or Forgejo PR: `series/milos-eswin-gesture-wake` |
| linux/milos | fp6/stk3310-bus-retry-local (local-only) | 63289f441152 | 1d46054f22d0 | - |
| ofono (depth 1) | fp6/provisiondb-pkgdatadir (2 patches) | master 00c3cb126746 | c2e6c2a7be06 | mail: `series/provisiondb-pkgdatadir` to ofono@lists.linux.dev |

Forks that exist but have nothing to send yet: **mir** (our input-method
patch is a backport of the merged canonical/mir PR #3000), **tqftpserv**
(we package upstream v1.2; the fix is to stop Mobile NixOS shadowing it),
**maliit-keyboard** (the panel-size-hint patch needs an agreed interface
with Lomiri first).
