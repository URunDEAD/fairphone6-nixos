# Both camera fixes with the unchanged exact4 Image/vermagic. Rebuild only the
# external CAMSS module; substitute its aggregated modules for initrd AND stage2.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/selfie-dphy
, baselineKernelBuild ? ../kernel-build/qcomtee-cma
# Kernel pin. The defaults are the exact DPHY kernel4; kernel5 targets
# (feature-push/trial-lomiri-full-kernel5-fp6.nix) pass their own values.
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
}:
let
  base = import ./trial-phosh-selfie-dphy-kernel4-fp6.nix {
    inherit nixpkgs kernelBuild baselineKernelBuild expectedKernelDrvPath kernelRelease;
  };
  exactKernel = import kernelBuild { system = "aarch64-linux"; };
  evaluated = base.eval.extendModules {
    modules = [ ({ lib, pkgs, ... }: let
      camss = pkgs.callPackage ./selfie-camss-kernel4.nix {
        kernel = exactKernel;
        inherit expectedKernelDrvPath kernelRelease;
      };
      modules = pkgs.aggregateModules [ exactKernel.modules camss ];
    in {
      # Mobile uses package.modules for its own initrd machinery. NixOS then
      # uses that same modules output for the stage2 aggregate. Image/dev/ABI
      # metadata and all kernel3/kernel4 input artifacts remain unchanged.
      mobile.boot.stage-1.kernel.package = lib.mkOverride 30 (exactKernel // {
        inherit modules;
      });
      # NixOS derives stage2 kernelPackages independently through linuxPackagesFor.
      # Include the exact replacement there explicitly; this adds no autoload.
      boot.extraModulePackages = [ camss ];
      system.build.fp6-camss-kernel4 = camss;
      system.build.fp6-camera-kernel4-modules = modules;
    }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
