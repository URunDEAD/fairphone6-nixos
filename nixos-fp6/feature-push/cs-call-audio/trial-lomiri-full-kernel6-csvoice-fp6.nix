# SUPERVISED CS-CALL-AUDIO TEST target (do not install; RAM boot only):
# trial-lomiri-full-kernel6-fp6.nix with the kernel6 recipe + kernel7
# candidates 0005-0008 (kernel-build/feature-push/kernel7-candidates/
# cs-call-audio-kernel) and hardware.fairphone6.csCallAudio enabled with the
# ACDB-derived voice graph. A copy of the kernel6 target as of 2026-10-27
# (keep its module list in sync by hand); see
# results/feature-push-20261024/cs-call-audio/README.md.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  kernelPin = {
    kernelBuild = ../../../kernel-build/feature-push/kernel7-candidates/cs-call-audio-kernel;
    expectedKernelDrvPath = "/nix/store/sn8ppqmc7xnfbdcz75a2ra9sz0y4skqz-linux-fp6-nixos-csvoice-7.2.0.drv";
    kernelRelease = "7.2.0-fp6-nixos6";
    # sha256 of the kernel6 builds (checked at runtime by the selfie boot-load unit):
    # fp6-s5kkd1.ko from s02slx2w...-fp6-s5kkd1-driver-diagnostic-0.3-kernel4-stream-reads,
    # updates/qcom-camss.ko from zc8hrrdl...-fp6-camss-kernel4-rdi16-1 (both vermagic nixos6).
    # Front camera Bayer fix (camera phase 0): the fp6-s5kkd1 module is the
    # same 0.3 source rebuilt with MEDIA_BUS_FMT_SGBRG10_1X10
    # (camera/selfie-gbrg-fp6.nix); the plain 0.3 build was
    # 6aa174a532756863a22fc83a8147f60bb6abe806ab53974606428c30579e2ef5.
    selfieModuleSha256 = "21cfae84945c73fd1f67f73db289dd6f87c2a4892f12e502eac0ec0829b34fda";
    expectedCamssSha256 = "7629d4dd284c12ed84f2a6ee63e4ae48a8444bb8b271986e25e33684d53ce8e7";
  };
  base = import ../trial-lomiri-full-fp6.nix { inherit nixpkgs kernelPin; };
  evaluated = base.eval.extendModules {
    modules = [ ../camera/selfie-gbrg-fp6.nix
      # Lomiri file picker for GTK4 (Fragments), Firefox and portal-using Qt
      # apps: xdg-desktop-portal FileChooser backend.
      # results/feature-push-20261024/file-picker/README.md
      ({ ... }: { imports = [ ../file-picker/file-picker-fp6.nix ]; hardware.fairphone6.filePickerPortal.enable = true; })
      # White-box text test (2026-10-27): GPU preemption off via msm.enable_preemption=0.
      # results/feature-push-20261024 white-boxes notes; the Mesa CCHE backport stays off.
      ({ ... }: { imports = [ ../white-boxes/white-boxes-fp6.nix ]; hardware.fairphone6.whiteBoxes.gpuPreemptionOff = true; })
      # OTA updates (Settings > System update). Slot switching OFF: installs
      # rewrite the running boot partition with a backup, no automatic rollback.
      # results/feature-push-20261024/ota/README.md
      ({ ... }: { imports = [ ../ota/ota-fp6.nix ]; hardware.fairphone6.ota.enable = true; hardware.fairphone6.ota.allowSlotSwitch = false; })
      ({ config, lib, ... }: let
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
      system.nixos.tags = [ "kernel6-csvoice" ];
      hardware.fairphone6.csCallAudio = {
        enable = true;
        dryRun = false;
        kernelHasVoiceSupport = true;
        voiceGraph = ./topology/milos-voice-graph.m4;
      };
      system.build.fp6-kernel6 = kernel;
      services.fp6SuspendPolicy.ufsLinkOffAtBoot = true;
      assertions = [{
        assertion = kernel.drvPath == kernelPin.expectedKernelDrvPath
          && kernel.modDirVersion == kernelPin.kernelRelease
          && config.boot.kernelPackages.kernel.modDirVersion == kernelPin.kernelRelease
          && config.system.build.fp6-camss-kernel4.passthru.kernelRelease == kernelPin.kernelRelease
          && config.system.build.fp6-selfie-kernel4-driver.passthru.kernelRelease == kernelPin.kernelRelease;
        message = "The cs-call-audio test target must use the csvoice kernel for the kernel and every out-of-tree module.";
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
