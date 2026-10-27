# Candidate mitigations for the "text turns into white boxes" bug (Lomiri/Mir
# on freedreno A810, Mesa 26.1.4, kernel 7.2). Every option defaults to false;
# nothing here is enabled without the coordinator. Capture script:
# tools/feature-push/white-boxes/capture.sh.
#
# Why text only: glyph atlases are almost the only GPU-written textures that
# persist across frames and get partial updates while the session runs
# (uploads through a staging buffer plus a 2D blit, and copies when the atlas
# grows). A transient GPU-side cache/coherency or preemption fault that hits
# one of those writes leaves a permanent mark. Per-frame render targets
# repaint the next frame. That would fit "only text, several processes at
# once, cured by a logout".
#
#  * mesaCcheInvalidateWfi: backport of Mesa main 5fcde4d65d8f (not in
#    26.1.x): CP_CCHE_INVALIDATE is a plain register write, so without a WFI
#    before it the CCHE (gen7.5+/gen8 texture-side cache) can be invalidated
#    before earlier writes (blits into a texture) land, and later texture
#    reads see stale lines. Rebuilds only the Mesa used as
#    hardware.graphics.package (no app rebuilds). It restates
#    ../../hardware-fp6.nix's override (same patch and meson flag) with
#    mkForce, because a module cannot extend another module's package value.
#  * gpuPreemptionOff: msm.enable_preemption=0 on the kernel command line.
#    a8xx preemption (a8xx_preempt.c) is new; with it on, any high-priority
#    GL/Vulkan context (Android SurfaceFlinger in Waydroid, maybe
#    Chromium/QtWebEngine) preempts every normal-priority client mid-batch.
#    New boot image, same kernel. The value shows in
#    /sys/module/msm/parameters/enable_preemption.
#
# Kernel-side candidate (not wired here; needs a kernel7 build and module
# re-pinning): kernel-build/feature-push/kernel7-candidates/
# 0004-drm-msm-a8xx-invalidate-cche-on-pagetable-switch.patch.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.whiteBoxes;
in {
  options.hardware.fairphone6.whiteBoxes = {
    mesaCcheInvalidateWfi = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Mesa freedreno: WFI before CP_CCHE_INVALIDATE (backport of mesa 5fcde4d65d8f).";
    };
    gpuPreemptionOff = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Boot with msm.enable_preemption=0 (single GPU ring, no a8xx preemption).";
    };
  };

  config = lib.mkMerge [
    (lib.mkIf cfg.mesaCcheInvalidateWfi {
      hardware.graphics.package = lib.mkForce (pkgs.mesa.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [
          ../../mesa-aarch64-os-features.patch
          ./mesa-freedreno-cche-invalidate-wfi.patch
        ];
        mesonFlags = (old.mesonFlags or [ ]) ++ [ "-Dlegacy-wayland=bind-wayland-display" ];
      }));
      system.build.fp6-white-boxes-mesa = config.hardware.graphics.package;
    })
    (lib.mkIf cfg.gpuPreemptionOff {
      boot.kernelParams = [ "msm.enable_preemption=0" ];
    })
  ];
}
