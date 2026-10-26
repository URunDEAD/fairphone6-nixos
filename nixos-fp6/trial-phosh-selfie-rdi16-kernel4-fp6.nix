# Isolated, matched exact4 successor. Preserve all earlier trial/fallback files.
# Both initrd and stage2 select the same new CAMSS module; clean RAM boot only.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/selfie-dphy
, baselineKernelBuild ? ../kernel-build/qcomtee-cma
# Kernel pin. The defaults are the exact DPHY kernel4; kernel5 targets
# (feature-push/trial-lomiri-full-kernel5-fp6.nix) pass their own values.
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
, selfieModuleSha256 ? "7860ee42ada71bac02eadae431f3accf55f117a9ae99ee152d4fc66688ac46e1"
, expectedCamssSha256 ? "00b8ba300e3ed7c5022c4384aa0a35e1ab845f69ce28eebbd4be4bb19ae7e7d8"
}:
let
  base = import ./trial-phosh-selfie-boot-enabled-kernel4-fp6.nix {
    inherit nixpkgs kernelBuild baselineKernelBuild expectedKernelDrvPath kernelRelease
      selfieModuleSha256 expectedCamssSha256;
  };
  exactKernel = import kernelBuild { system = "aarch64-linux"; };
  evaluated = base.eval.extendModules {
    modules = [ ({ lib, pkgs, ... }: let
      camss = pkgs.callPackage ./selfie-camss-kernel4-rdi16.nix {
        kernel = exactKernel;
        inherit expectedKernelDrvPath kernelRelease;
      };
      modules = pkgs.aggregateModules [ exactKernel.modules camss ];
    in {
      mobile.boot.stage-1.kernel.package = lib.mkOverride 20 (exactKernel // {
        inherit modules;
      });
      # Replace the prior CAMSS entry, rather than aggregating two versions with
      # the same module filename. No other external module exists in this trial.
      boot.extraModulePackages = lib.mkForce [ camss ];
      system.build.fp6-camss-kernel4 = lib.mkForce camss;
      system.build.fp6-camera-kernel4-modules = lib.mkForce modules;
    }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
