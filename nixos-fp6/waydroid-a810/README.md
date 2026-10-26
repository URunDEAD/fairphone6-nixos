# A810 Android Mesa: explicit fresh trials

This package supplies one matched set of **seven Android API 33 AArch64 Bionic
libraries** from pinned sources. It does not activate Android, change an image,
or access existing Android userdata. Physical GPU acceptance remains outstanding.

`source.nix` builds Mesa 26.1.4 and libdrm 2.4.123 using native Nix Clang/LLD
21.1.8 and the official NDK r27d target data. Compiler intrinsic headers match
Clang 21; Android sysroot, static libc++ and compiler-rt come from the pinned NDK.
No NDK x86 executable runs. Sources are hash-pinned, Meson downloads disabled,
and all helpers/configuration are authoritative package-local files under
`build-support/` and `trial/`. No recipe depends on a results or private-log tree.
Only build-script interpreter paths are patched; driver sources are unchanged.
Native dependencies use this project's pinned nixpkgs, with two compiler jobs.

The full source build **passed** on the native ARM VM in 135 seconds. Its seven
libraries independently passed actual Android 13 strong-symbol/version closure,
all 36 saved runtime file hashes, actual baseline GBM imports and native Bionic
`RTLD_NOW` loading (seven loaded, zero failures, no graphics API calls). This is
build/ABI/loader evidence, not physical GPU or complete Android namespace proof.

The default `.runtime` derivation copies and revalidates the seven source-built
libraries plus portable manifests/license. It is approximately 36.8 MiB and has
**zero runtime store references**; compiler/NDK audit metadata stays in `.source`
on the build host. The module and `.tools` both select `.runtime` by default.
The runtime subset itself also passed the same ABI/Bionic loader checks.

`binary.nix` retains the earlier independently validated Clang 18 artifact as an
explicit fallback. The caller must provide its external archive; the 8.1 MiB
archive hash and every individual library hash remain pinned. It uses pristine
Mesa/libdrm, Ubuntu Clang/LLD 18.1.3, NDK r27d and Meson 1.8.5; `provenance.json`
records this distinct history. It is a binary repack, not the Nix source build.

Build from the project root on native aarch64 Linux:

```sh
nix-build nixos-fp6/waydroid-a810 -A runtime --max-jobs 1 --cores 2
nix-build nixos-fp6/waydroid-a810 -A tools --max-jobs 1 --cores 2
nix-instantiate --eval --strict --json nixos-fp6/waydroid-a810/eval-checks.nix
# Optional configure/link smoke only, or full build with retained audit metadata:
nix-build nixos-fp6/waydroid-a810 -A sourceConfigure --max-jobs 1 --cores 2
nix-build nixos-fp6/waydroid-a810 -A source --max-jobs 1 --cores 2
# Explicit independent binary fallback:
nix-build nixos-fp6/waydroid-a810 -A binary \
  --arg binaryArchive /path/fp6-mesa-26.1.4-a810-vendor-libs.tar.gz \
  --max-jobs 1 --cores 2
```

Missing cached source inputs would fetch about 634 MiB of official NDK data and
75 MiB of Mesa source. Existing trusted archives seed the fixed-output store
without network transfer:

```sh
nix-store --add-fixed sha256 /path/android-ndk-r27d-linux.zip \
  /path/mesa-26.1.4.tar.xz /path/libdrm-2.4.123.tar.xz
```

No SDK manager, AOSP tree or vendor image is a build input. Baseline GBM symbol
names are plain JSON; vendor binaries are not needed to build. Packages never
apply host ELF fixups, stripping or RPATH rewriting to the Bionic libraries.

Repeat the bounded ABI/loader acceptance on a native ARM Linux host using the
previously hash-verified public Android 13 runtime and vendor consumer ELF files:

```sh
python3 nixos-fp6/waydroid-a810/build-support/validate-nix-source-result.py \
  --package /nix/store/ACTUAL-runtime-output \
  --toolchain /nix/store/ACTUAL-source-output/share/fp6-waydroid-a810/toolchain.json \
  --runtime /path/verified-android-runtime \
  --consumers /path/verified-vendor-consumers --output /path/new-check-output
```

This executes the saved Bionic loader only, never starts Android or calls GPU
APIs. Android namespace permissions, buffer ABI and hardware need separate tests.

## Optional NixOS exposure

The module defaults to disabled. Enable it only in the separately reviewed trial
system, with the stock Waydroid module explicitly disabled:

```nix
imports = [ ./waydroid-a810.nix ];
virtualisation.waydroid.enable = lib.mkForce false;
hardware.fairphone6.androidMesaTrial.enable = true;
```

Enabling exposes `.runtime` as `/etc/fp6-waydroid-a810`, adds three explicit trial
commands and makes the LXC CLI available for exact-name recovery. It adds no
service, timer, activation script, image mount, binder provisioning, bridge or
auto-start action. Evaluation checks passed default-off/opt-in inertness and
rejection of a stock Waydroid conflict. To select the independent fallback:

```nix
hardware.fairphone6.androidMesaTrial.package =
  (import ./waydroid-a810 {
    inherit pkgs;
    binaryArchive = /path/fp6-mesa-26.1.4-a810-vendor-libs.tar.gz;
  }).binary;
```

The seven paths under `lib64/` are:

- `libgallium_dri.so`, `libgbm_mesa.so`, `dri_gbm.so`
- `egl/libEGL_mesa.so`, `egl/libGLESv1_CM_mesa.so`, `egl/libGLESv2_mesa.so`
- `hw/vulkan.freedreno.so`

Keep all seven together. Preserve the baseline gralloc modules, GBM wrapper,
HWC, media configuration and the existing OMX-removal VINTF manifest. Neither
package supplies stub Android libraries, replacements for these components, nor
hardware MediaCodec support.

## Activation is a separate fresh trial

Do not start a phone trial until a physically recoverable, newly booted kernel
has healthy GPU state, binderfs and `/dev/dma_heap/system`. The current wedged
GPU is not a valid acceptance target. Keep the stock Waydroid service stopped.
A trial can display an Android surface on the **explicitly selected compositor**;
coordinate that runtime step independently from package installation.

The preparer pins actual baseline system/vendor image hashes (see provenance),
copies both images to a new private root and verifies both copies. It creates
new empty private userdata, stages exactly seven matched libraries and preserves
the OMX-removal manifest. It never calls `waydroid init`, mounts the original
images, or reads any existing Android userdata.

First inspect its plan with the real non-root compositor UID and owned socket:

```sh
sudo fp6-waydroid-prepare-trial \
  --trial-root /var/lib/fp6-waydroid-trials/fp6-a810-001 \
  --system-image /run/nura/etc/waydroid-extra/images/system.img \
  --vendor-image /run/nura/etc/waydroid-extra/images/vendor.img \
  --mode phone-gpu --host-uid COMPOSITOR_UID \
  --wayland-socket /run/user/COMPOSITOR_UID/ACTUAL_SOCKET --plan
```

Remove only `--plan` to prepare. Review the new `trial.json`, config and hashes:

```sh
sudo fp6-waydroid-review-trial /var/lib/fp6-waydroid-trials/fp6-a810-001
# Only after the separate runtime review and physical recovery gate:
sudo fp6-waydroid-run-trial /var/lib/fp6-waydroid-trials/fp6-a810-001 --start-180s
```

The direct named LXC trial uses a private mount namespace, three dedicated
binder contexts,2 GiB RAM/512 MiB swap/2 CPU, no host network and no audio/input/KMS/
framebuffer/video devices. Phone mode exposes only renderD128 and system DMA
heap. VM software mode exposes neither and requires at least 5 GiB available RAM.
The runner waits up to180 seconds for boot, saves bounded diagnostics, then
stops the exact container, waits for its foreground process, and checks all
observed PIDs, payload cgroups and private mounts are gone. Treat any failed
`cleanup.json` as a cleanup failure, retaining its exact resource IDs for review.
Never reuse a trial root; the runner requires fresh data and a one-run marker.

Rollback: a normal trial already stops in `finally`; confirm its `cleanup.json`
is PASS. On interruption, inspect and stop only the exact private container:

```sh
sudo lxc-stop -P /var/lib/fp6-waydroid-trials/fp6-a810-001/lxc \
  -n fp6-a810-001 -k
sudo lxc-info -P /var/lib/fp6-waydroid-trials/fp6-a810-001/lxc \
  -n fp6-a810-001 -sH
```

Do not delete a trial until its recorded PIDs/cgroup/mounts are confirmed gone.
Keep logs private; retain failed trials for diagnosis. No restoration of live
images/userdata is required because none were changed. Disable the option and
rebuild the previously reviewed system to remove the exposure/tools. This does
not delete retained trial data or alter any live Android installation.

## Evidence and remaining gates

Actual phone GPU/GLES/Vulkan drawing, hardware activity, stable GMU recovery,
Android namespace/buffer allocation and repeated app transitions remain untested.
The eventual hardware gate requires Adreno 810 renderer plus real rendered
pixels, GPU activity and no new kernel hangs; `boot_completed=1` is insufficient.
Hardware Iris MediaCodec needs a separate API 33-compatible codec HAL and real
frame tests; these graphics libraries do not provide it. Network, audio,
clipboard, telephony and complete host session integration remain separate.

Fresh native ARM VM Android boot and built-in Settings rendering passed:
ANGLE/SwiftShader pixels equalled the actual Weston Pixman output byte-for-byte
at 720×1280, with private fresh userdata and complete runtime cleanup. That test
proves Android→Wayland software render plumbing, not these Freedreno libraries,
physical Adreno execution, hardware video or networking. The VM's signed stock
binder module lacks an unload entry point and remains idle until a planned VM
reboot; no binder mounts/contexts or Android containers remain active.
