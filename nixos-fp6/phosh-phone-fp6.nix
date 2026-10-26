# Current phone work: original native apps, mobile Firefox, camera userspace,
# persistent GPU Waydroid and explicit read-only fingerprint initialization.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/qcomtee-cma
}:
let
  base = import ./firefox-mobile-phosh-waydroid.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./fingerprint-readonly-fp6.nix ./security-tools-module.nix ({ pkgs, ... }: {
      hardware.fairphone6.postmarketosApps.extraPackages = [
        (pkgs.callPackage ./postmarketos-apps-chatty.nix {})
      ];
      services.fp6FingerprintReadOnly.enable = true;
      services.fp6FingerprintReadOnly.package =
        pkgs.callPackage ./fingerprintd-transport-package.nix {};
      hardware.fairphone6.securityTools.enable = true;
    }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
