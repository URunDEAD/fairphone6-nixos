FP6 kernel feature build and validation, 2026-10-23

STATUS
The native ARM Linux build and package validation passed. No persistent phone
writes or RAM boot of this new kernel have been performed by this builder.
The original device/kernel.nix and all nixos-fp6/assets remain unchanged.
A separate trial system/image built successfully against this exact kernel.
Generic AppArmor, binderfs and DMA-heap runtime probes passed on QEMU virt;
physical FP6 boot, hardware and GPU-buffer import remain unverified.

SOURCE
https://forgejo.catcrafts.net/Catcrafts/milos-linux.git
Commit 63289f4411520a2acb38bd533437446d33fbce74, 7.2.0
source/ is a clean detached shallow Git checkout of that exact commit.
The portable source archive was created with:
  git -C source archive --format=tar --prefix=linux-63289f4411520a2acb38bd533437446d33fbce74/ HEAD | gzip -n > linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz
Archive bytes: 266045328
SHA256: 1a1aba170899273cbb97bce2dc0713957340a7be2e2d187faab6f821d60687ba
default.nix asserts that hash before building.

CONFIGURATION
features.config overlays the entire known-working assets/kernel.config.
config.final was resolved with the pinned nixpkgs LLVM 21.1.8 toolchain.
config-diff.txt records every difference from the original Alpine Clang23.1.3
build. Three compiler capability probes disappear with LLVM21; every original
enabled driver and other non-toolchain kernel feature was retained. The change
uses a unique module release 7.2.0-fp6-nixos1 to prevent mixing old modules.
ThinLTO, the device-specific drivers, all original hardening and boot settings
are retained. Re-run configuration with:
  nix-shell kernel-build/shell.nix --run 'bash kernel-build/prepare-config.sh'

Added: SECURITY_APPARMOR plus audit/securityfs/path/network dependencies,
AppArmor policy introspection/hash/export/verification defaults, AppArmor in
CONFIG_LSM, ANDROID_BINDERFS, and DMABUF_HEAPS_SYSTEM. Existing Binder IPC,
MEMFD_CREATE and fixed binder/hwbinder/vndbinder devices remain enabled.
The system heap supplies /dev/dma_heap/system, useful for Android GPU buffer
allocation; successful GPU allocation must still be verified with the vendor.

BUILD ON THE ARM LINUX VM
Transfer default.nix, config.final and the source archive into kernel-build/.
Run from its parent:
  nix-build kernel-build --max-jobs 1 --cores 4 --keep-failed -o result-kernel-fp6
An existing exact pinned nixpkgs tree may avoid fetching it again:
  nix-build kernel-build --arg nixpkgs /path/to/pinned-nixpkgs -o result-kernel-fp6
The default system is aarch64-linux and compiler is pinned LLVM21.1.8.
The completed build retained ThinLTO and limits linker ThinLTO jobs to 2.
The recipe explicitly selects LLVM target tools: upstream LLVM=1 alone does
not override Nixpkgs' absolute GNU tool flags. Host Clang uses the wrapped host
linker to retain OpenSSL RPATH. With CONFIG_EFI_ZBOOT=y, KBUILD_IMAGE must be
explicitly Image.gz during install; otherwise zinstall requests vmlinuz.efi.
A synthetic-artifact Nix build first exercised the real install/postInstall/
fixup path, including DTB installation, compressed modules, dev headers and
module-directory symlinks. This preflight output is not a bootable kernel.
The subsequent complete production compilation/install/fixup passed.

EXACT PRODUCTION OUTPUTS
Derivation:
  /nix/store/60hby8nmax1f92w01qiwqcllr5zsj0xa-linux-fp6-nixos-7.2.0.drv
Kernel and DTBs:
  /nix/store/645vp7098dq9i0knnvjvhl4ik3v8m4sv-linux-fp6-nixos-7.2.0
Modules:
  /nix/store/a9pya2cpzwcc8k09j86bvznrbspgxbs8-linux-fp6-nixos-7.2.0-modules
Development headers:
  /nix/store/qhj0dlh6zv0m37dcp4cyh8zaqj3z0ic5-linux-fp6-nixos-7.2.0-dev
Image.gz: 12587495 bytes
SHA256: 0b532953daea0c2f053cba963fb817b28554f69c16974903e6627c423ebb3a02
DTB: dtbs/qcom/milos-fairphone-fp6.dtb (118526 bytes)
Modules release: 7.2.0-fp6-nixos1; never mix original 7.2.0 modules.

ARTIFACT VALIDATION PASSED
Decompressed Image has the AArch64 ARMd header and expected release. Embedded
IKCONFIG exactly matches config.final; AppArmor, binderfs, system DMA heap,
ThinLTO, LLD and IKCONFIG_PROC are enabled. FP6 DTB identifies The Fairphone
(Gen. 6), fairphone,fp6 and qcom,milos. All 417 installed module files are zstd
compressed; sampled crypto and IMU modules have the matching release/aarch64
vermagic. Development .config matches the embedded configuration. Evidence:
  .macmini-builder/kernel-validation.json
  .macmini-builder/kernel-build.log
Guest build workdir: ~/fp6-isolated-builds/kernel-app-binder
Its exit-status is 0. The package checks do not substitute for a RAM boot.



EXACT KERNEL GENERIC CORE RUNTIME TEST PASSED (MAIN AGENT)
The exact production Image.gz SHA0b532953... booted on isolated QEMU virt with
a private3.5MB initramfs. Release7.2.0-fp6-nixos1 matched. Private binderfs
created3 devices and reported protocol8. System DMA heap allocated4096 bytes,
with mmap/write/read and DMA_BUF_IOCTL_SYNC passing. An AppArmor profile
loaded, allowed a control read, denied a synthetic read with Permission denied,
and was removed. Guest powered off, QEMU unit inactive/success/status0.
This verifies generic core facilities in this exact binary, not FP6 platform
boot, drivers, graphical startup, GPU DMA import or Android on physical FP6.
Evidence: results/kernel-qemu/{boot.log,manifest.json,result.json,packages-and-exit.txt}.

FINAL NATIVE QT UI SYSTEMS AND RAM IMAGES
All source compilation, upstream checks, system assembly and artifact checks
passed. Source/default configuration files remain separately owned. The native
Qt5 scope reuses patched Qt849 and rebuilds consumers with final install paths;
no binary dependency replacement or process library override is required.

OLD kernel native Qt system:
  /nix/store/y6369lz061iy0hykshpnnpnhdpsp7p9j-nixos-system-fp6-26.11pre1031299.0bb7ec54c848
OLD kernel CPU recovery system (corrected drop-in):
  /nix/store/k1vr7vywyzjs7ynzp9grf3q8886yaadi-nixos-system-fp6-26.11pre1031299.0bb7ec54c848
NEW kernel UI plus reviewed Android trial tools:
  /nix/store/w9ik3yda5134srp4l6c0vzkwwr4vrgz8-nixos-system-fp6-26.11pre1031299.0bb7ec54c848

The NEW UI variant imports manual brightness, APPS IIO proxy ordering/anti-SSC
rule, short-power-key ignore and the reviewed A810 trial module. Brightness
starts with multi-user.target after separate live acceptance. Torch permissions
affect only continuous brightness. Stock Waydroid is DISABLED and its container
unit absent. Fresh Android tools/runtime are installed without any automatic
container/image initialization/start/network setup. This preserves the base
trial-kernel.nix default graph: its default derivation was verified unchanged
when making the empty stock-container wantedBy conditional.

Both kernel stages use exact60hby/645, only module release7.2.0-fp6-nixos1,
with apparmor=1 and lsm=landlock,yama,apparmor,bpf. boot.json points to the exact
NEW root/kernel/init; its initrd contains matching modules. Proper Mesa xwv
retains legacy Wayland and masks LLVM CPU features using AArch64 OS HWCAP.
This kernel adds AppArmor/binderfs/systemDMAheap; it contains no newly proven
physical GPU or natural-resume fix.

GENERATION-PINNED RAM IMAGES, OUTSIDE NIX STORE
Each image is25,747,456 bytes, mode0600. Both explicitly set
mobile-nixos.generation to the exact NEW w9ik system above. The normal image
selects the normal target; the console-first image adds only
systemd.unit=multi-user.target. This avoids normal greeter startup before
controlled tests; it does not exclude earlier initrd DRM access.

Normal UI:
  Guest: /home/claude.guest/fp6-isolated-builds/kernel-ui-native-a810/deployment-w9ik3yda5134srp4l6c0vzkwwr4vrgz8.img
  Local: .macmini-builder/native-final/new/deployment-w9ik3yda5134srp4l6c0vzkwwr4vrgz8.img
  SHA256: 1d798860df50786a710083fce5e4f57ecba09eb721af54da67f703c76a62dd3d

Console first:
  Guest: /home/claude.guest/fp6-isolated-builds/kernel-ui-native-a810/deployment-w9ik3yda5134srp4l6c0vzkwwr4vrgz8-console.img
  Local: .macmini-builder/native-final/new/deployment-w9ik3yda5134srp4l6c0vzkwwr4vrgz8-console.img
  SHA256: c47468e0962287011199353782bbf91c1827b8627fe833c709089989ebec7f5b

Kernel, initrd and DTB are byte-identical across both images. All three SHA256
hashes, Android boot ID, page/header format, exact command lines, non-overlapping
load ranges and bootspec/module consistency passed validation. The images
differ only in Android command-line fields. Full component hashes/loadaddresses
are in .macmini-builder/native-final/new/final-artifacts.json.

The complete NEW system closure must be imported before an approved RAM boot.
The generation pin selects matching stage2 without changing the persistent
system profile. Earlier unpinned c344/raw Nix images must not boot while the
profile selects an original7.2.0 kernel: that would mix module releases.
Known boot-normal.img and boot-recovery-k3.img hashes were checked unchanged.
No phone profile change, activation, RAM boot or partition flash was performed
by this builder. Main agent owns any later phone action.

NATIVE QT AND SERVICE VALIDATION
OLD/CPU/NEW full runtime scans found zero UTF16 store references outside each
closure (7159/7159/7180 ELF files). Stock Qt5 declarative is absent; QtQuick/Qml/
QmlModels and plugins resolve Qt849 by RPATH. Native keyboard's UTF16 QML and
language directories exist, its wrappers select its real AArch64 ELF, and the
QtMir whitelist matches that exact binary. Required QML/plugin paths exist.
Only API-library plugin-prefix and test-mocks fallback directories are absent;
these optional non-provider paths are source-verified and reported explicitly.
Lomiri installed Shell/Error QML paths and equal-length relocation regression
passed with its actual generated paths.h helpers. All final roots preserve
Lomiri's vendor Type=notify service and usable native session ExecStart.

CPU44 was rejected by live acceptance: its environment-only complete unit
shadowed the vendor lomiri.service without ExecStart. CPU1nn restored the vendor unit with asDropin, but live testing then found
its generated minimal PATH omitted D-Bus activation, gdbus and Xwayland. Current
CPUk1 uses asDropin plus config.system.path. The actual unit/wrapper PATH resolves
all12 required commands and preserves the complete installed bin/sbin profile;
the same audit rejects1nn with exactly those3 commands missing. Only4 text
unit/root outputs changed, with every ELF/resource package unchanged. Historical
incomplete evidence under cpu-rejected44 and cpu-rejected1nn is not readiness
proof. Parent live k1 acceptance passed login/PAM,2 OSK hide/show/edit cycles,
normal-GL child200x120=>physical600x360 and TerminalPreferences with Qt849. The normal y636 and NEW w9ik systems/images were unaffected.

Complete evidence and exact diagnostic qmlscene paths:
  .macmini-builder/native-final/{old,cpu,new}/
  .macmini-builder/native-final/local-artifact-proof.json
  results/qt-borderimage/{review.txt,qt-diagnostic-paths.json}
  results/llvmpipe-jit/phone-patched-mesa-no-preload.txt
The Mesa CPU fallback child separately read64/64 red pixels on phone without
LD_PRELOAD/SIGILL. Neither that result nor generic QEMU validates physical FP6
new-kernel graphics, resume, GPU DMA import or Android hardware behavior.

REPEAT BUILDS ON THE EXISTING ARM GUEST
  nix-build ~/fp6-isolated-builds/kernel-ui-native-a810/build.nix -A system -A bootImage --max-jobs 1 --cores 2
  nix-build ~/fp6-native-qt5-production-cpu/default.nix -A system --max-jobs 1 --cores 2
These immutable artifacts are already built. Repacking generation-pinned images
is a separate outside-store step to avoid a Nix root/image dependency cycle.

OLD-KERNEL ACCEPTED CPU RECOVERY RAM IMAGE
A separate outside-store image pins the accepted native CPUk1 root while
retaining the protected boot-normal image's kernel/DTB/initrd byte-for-byte.
The CPU target's own Nix-built raw boot image exactly equals protected
boot-normal SHA80e7a911149e479debf067f1a79cf395f86ccc0ee72bb0bb69b925f962f8947f.
Thus both stages/modules remain original7.2.0 and all components are compatible.
Host mode0600 image:
  .macmini-builder/native-final/cpu/deployment-k1vr7vywyzjs7ynzp9grf3q8886yaadi-recovery.img
SHA256: 7a0559d7ee4610a794217ed270ef4520ee6533785c9b0a01304300e60c700cbe
Bytes:25587712. Only Android command-line fields differ. All3 component hashes,
Android boot ID, header/load addresses and non-overlap checks pass;27 matching
initrd module entries and compiled generation-selection token found. Exact
report is adjacent .img.json; make-cpu-recovery-image.py is in .macmini-builder.
Import the complete k1 closure before a future approved RAM boot. The image
does not change the persistent profile. Protected normal/recoveryk3 images
remain unchanged. This recovery image has not been booted by this builder.
