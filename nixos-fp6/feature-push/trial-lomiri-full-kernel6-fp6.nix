# The full-features Lomiri GPU target (trial-lomiri-full-fp6.nix) on kernel6:
# kernel-build/feature-push/kernel6 = exact kernel5 recipe plus the UFS
# system-suspend fix for milos (kernel-build/feature-push/suspend/0001-...:
# "qcom,milos-ufshc" gets no_phy_retention -> default spm_lvl 5, as upstream
# does for SM8550/SM8650) and CONFIG_PM_DEBUG (/sys/power/pm_test for the
# staged, supervised first suspend test). modDirVersion 7.2.0-fp6-nixos6.
# Driver/config-only: the separately built, preserved DTB is unchanged.
# Suspend stays GUARDED (AllowSuspend=no); udev also writes spm_lvl=5.
# See results/feature-push-20261024/suspend/README.md.
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  kernelPin = {
    kernelBuild = ../../kernel-build/feature-push/kernel6;
    expectedKernelDrvPath = "/nix/store/g53ral2ypy7l6p5v943yngh12bi26qms-linux-fp6-nixos-7.2.0.drv";
    kernelRelease = "7.2.0-fp6-nixos6";
    # sha256 of the kernel6 builds (checked at runtime by the selfie boot-load unit):
    # fp6-s5kkd1.ko from s02slx2w...-fp6-s5kkd1-driver-diagnostic-0.3-kernel4-stream-reads,
    # updates/qcom-camss.ko from zc8hrrdl...-fp6-camss-kernel4-rdi16-1 (both vermagic nixos6).
    selfieModuleSha256 = "6aa174a532756863a22fc83a8147f60bb6abe806ab53974606428c30579e2ef5";
    expectedCamssSha256 = "7629d4dd284c12ed84f2a6ee63e4ae48a8444bb8b271986e25e33684d53ce8e7";
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
      # Top mic on: UCM reads ADC2 from SWR_MIC0 (results/feature-push-20261024/audio/mic2-README.md).
      hardware.fairphone6.audioFeature.topMic = true;
      # kernel6 (like kernel5) carries the ESWIN lp_mode patch.
      services.fp6LomiriSensors.doubleTapToWake = true;
      system.nixos.tags = [ "kernel6" ];
      system.build.fp6-kernel6 = kernel;
      services.fp6SuspendPolicy.ufsLinkOffAtBoot = true;
      assertions = [{
        assertion = kernel.drvPath == kernelPin.expectedKernelDrvPath
          && kernel.modDirVersion == kernelPin.kernelRelease
          && config.boot.kernelPackages.kernel.modDirVersion == kernelPin.kernelRelease
          && config.system.build.fp6-camss-kernel4.passthru.kernelRelease == kernelPin.kernelRelease
          && config.system.build.fp6-selfie-kernel4-driver.passthru.kernelRelease == kernelPin.kernelRelease;
        message = "The kernel6 full target must use kernel6 for the kernel and every out-of-tree module.";
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
