# The full-features Lomiri GPU target (trial-lomiri-full-fp6.nix) on kernel5:
# kernel-build/feature-push/kernel5 = exact kernel4 (selfie-dphy) recipe plus
# the 2026-10-24 driver-only patches (battmgr uAh, stk3310 no-IRQ + retry,
# eswin gesture-wake lp_mode). modDirVersion 7.2.0-fp6-nixos5.
#
# Everything kernel-specific comes from the kernel4 chain's pin parameters
# (all defaulting to kernel4, so kernel4 targets evaluate unchanged):
#  * stage-1/stage-2 kernel = kernel5, with the rdi16 CAMSS external module
#    and the fp6-s5kkd1 diagnostic driver rebuilt against kernel5's -dev tree;
#  * the selfie boot-load unit checks kernel5's uname/vermagic and the
#    kernel5 module hashes below;
#  * DTB: the same separately built, preserved DTB from baselineKernelBuild
#    (qcomtee-cma) as kernel4. The kernel5 patches touch no DT, so boot_b's
#    DTB bytes stay the same;
#  * boot image: the same selfie repack (header v2), with kernel5 Image.gz
#    and mobile-nixos.generation= pinning this toplevel.
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  kernelPin = {
    kernelBuild = ../../kernel-build/feature-push/kernel5;
    expectedKernelDrvPath = "/nix/store/rvkydyrqsz90pnm82myqcvh8jmlgbwz4-linux-fp6-nixos-7.2.0.drv";
    kernelRelease = "7.2.0-fp6-nixos5";
    # sha256 of the kernel5 builds (checked at runtime by the selfie boot-load unit):
    # fp6-s5kkd1.ko from z1dk1608...-fp6-s5kkd1-driver-diagnostic-0.3-kernel4-stream-reads,
    # updates/qcom-camss.ko from 0bb75jmi...-fp6-camss-kernel4-rdi16-1 (both vermagic nixos5).
    selfieModuleSha256 = "147d3408d90c41c6f77567848b8aff55b12dc552556399b2b6f5f085e3ece0db";
    expectedCamssSha256 = "a7e46ebe1c6fd0d46ece7a41a4f6a13b6a161769a577e2867ec59c02cc3758f5";
  };
  base = import ./trial-lomiri-full-fp6.nix { inherit nixpkgs kernelPin; };
  evaluated = base.eval.extendModules {
    modules = [ ({ config, lib, ... }: let
      kernel = config.mobile.boot.stage-1.kernel.package;
    in {
      # stk3310 (STK36C61 ALS/proximity, CONFIG_STK3310=m) autoloads through
      # its OF modalias (of:...Csensortek,stk36c61Csensortek,stk3310; bound by
      # udev on kernel4). Listing it as well makes the load deterministic even
      # if the i2c-gpio bus appears after the coldplug.
      boot.kernelModules = [ "stk3310" ];
      # Top mic on: Mic2 UCM reads ADC2 from SWR_MIC0 (SoundWire master channel
      # quirk; results/feature-push-20261024/audio/mic2-README.md).
      hardware.fairphone6.audioFeature.topMic = true;
      # kernel5 carries the ESWIN lp_mode patch.
      services.fp6LomiriSensors.doubleTapToWake = true;
      system.nixos.tags = [ "kernel5" ];
      system.build.fp6-kernel5 = kernel;
      assertions = [{
        assertion = kernel.drvPath == kernelPin.expectedKernelDrvPath
          && kernel.modDirVersion == kernelPin.kernelRelease
          && config.boot.kernelPackages.kernel.modDirVersion == kernelPin.kernelRelease
          && config.system.build.fp6-camss-kernel4.passthru.kernelRelease == kernelPin.kernelRelease
          && config.system.build.fp6-selfie-kernel4-driver.passthru.kernelRelease == kernelPin.kernelRelease;
        message = "The kernel5 full target must use kernel5 for the kernel and every out-of-tree module.";
      }];
    }) ];
  };
in base // {
  eval = evaluated;
  inherit kernelPin;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
