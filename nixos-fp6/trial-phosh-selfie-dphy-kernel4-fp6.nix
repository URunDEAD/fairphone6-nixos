# Separate matched4 trial. No kernel3 source/target changes or sensor autoload.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/selfie-dphy
, baselineKernelBuild ? ../kernel-build/qcomtee-cma
# Kernel pin. The defaults are the exact DPHY kernel4; kernel5 targets
# (feature-push/trial-lomiri-full-kernel5-fp6.nix) pass their own values.
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
}:
let
  base = import ./phosh-phone-fp6.nix { inherit nixpkgs kernelBuild; };
  # The exact same source-built, preserved-symbol DTB as the proven3 candidate.
  baselineKernel = import baselineKernelBuild { system = "aarch64-linux"; };
  repacker = assert builtins.hashFile "sha256" ./selfie-fp6-boot-dtb/repack-new-kernel.py
    == "062f49e196504c288f0d43bbfbe6cc7d15356e3096b8e327ae67f91fd936da62";
    ./selfie-fp6-boot-dtb/repack-new-kernel.py;
  evaluated = base.eval.extendModules {
    modules = [ ./fingerprint-preserved-fp6.nix ({ config, lib, pkgs, ... }: let
      kernel = config.mobile.boot.stage-1.kernel.package;
      driver = pkgs.callPackage ./selfie-fp6-diagnostic-driver-kernel4.nix {
        inherit kernel expectedKernelDrvPath kernelRelease;
      };
      firmware = pkgs.callPackage ./selfie-fp6-firmware.nix {};
      dtb = pkgs.callPackage ./selfie-fp6-boot-dtb.nix { kernel = baselineKernel; };
      cmdline = lib.concatStringsSep " " (config.boot.kernelParams ++ [
        "mobile-nixos.generation=${config.system.build.toplevel}"
      ]);
    in {
      assertions = [ {
        assertion = kernel.drvPath == expectedKernelDrvPath
          && config.boot.kernelPackages.kernel.drvPath == kernel.drvPath
          && kernel.modDirVersion == kernelRelease
          && config.mobile.system.android.appendDTB == null
          && config.mobile.system.android.bootimg.dt == null
          && lib.all (p: !(lib.hasPrefix "mobile-nixos.generation=" p)) config.boot.kernelParams;
        message = "The matched4 selfie trial requires exact DPHY kernel4, separate preserved DTB and a single generation pin.";
      } ];
      hardware.fp6Camera.package = pkgs.callPackage ./selfie-fp6-libcamera.nix {};
      # Specific OEM register-data package only, retaining its original license.
      nixpkgs.config.allowUnfreePredicate = lib.mkDefault
        (p: lib.getName p == "fp6-s5kkd1-firmware");
      environment.systemPackages = [ driver ];
      hardware.firmware = [ firmware ];
      environment.etc."fp6-selfie-kernel4-diagnostic".source = "${driver}/share/fp6-selfie-diagnostic";
      system.build.fp6-selfie-kernel4-driver = driver;
      system.build.fp6-selfie-kernel4-firmware = firmware;
      system.build.fp6-selfie-boot-dtb = dtb;
      services.fp6FingerprintReadOnly.enable = lib.mkForce false;
      services.fp6FingerprintPreserved = {
        enable = true;
        mode = "read-only";
        startAtBoot = false;
        allowExplicitRemoval = false;
        standbyUnlock = false;
        enrollmentAndMatchVerified = false;
      };
      mobile.outputs.android.android-bootimg = lib.mkOverride 30
        (pkgs.runCommand "fp6-nixos-selfie-dphy-kernel4-trial.img" {
          nativeBuildInputs = [ pkgs.python3 ];
        } ''
          python ${repacker} ${./assets/boot-template.img} ${kernel}/Image.gz \
            ${config.mobile.outputs.initrd} ${dtb} $out \
            --cmdline ${lib.escapeShellArg cmdline}
        '');
      # No boot.extraModulePackages/kernelModules/udev/modprobe/stream changes:
      # the sensor module is installed as data for an explicit parent trial.
    }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
