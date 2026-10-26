# Lomiri + Mir (GPU, freedreno/Adreno 810) with the Ubuntu Touch default app
# set, as a NixOS target. Extends the exact chain of the running generation
# y2959, which was built from trial-phosh-selfie-rdi16-kernel4-fp6.nix
# (-> boot-enabled -> csid -> dphy -> phosh-phone-fp6 ...): DPHY kernel4
# 7.2.0-fp6-nixos4 + rdi16 CAMSS module, separate preserved DTB, selfie repack,
# camera and read-only fingerprint. (results/fp6-selfie-stride-review/
# target-evaluation.json records y2959 as that file's toplevel.)
# Building never activates anything or writes a phone partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs
# Kernel pin forwarded to the rdi16 chain. Empty = that file's kernel4 defaults
# (exact y2959 chain); ../trial-lomiri-full-kernel5-fp6.nix passes kernel5.
, kernelPin ? {}
}:
let
  kernelRelease = kernelPin.kernelRelease or "7.2.0-fp6-nixos4";
  base = import ../../trial-phosh-selfie-rdi16-kernel4-fp6.nix ({ inherit nixpkgs; } // kernelPin);
  evaluated = base.eval.extendModules {
    modules = [
      {
        # phosh-fp6.nix asserts Phosh is the only session owner and
        # waydroid-fp6-production.nix asserts Phosh; both are replaced by
        # Lomiri equivalents (see lomiri-gpu-fp6.nix / waydroid-fp6-lomiri.nix).
        disabledModules = [ ../../phosh-fp6.nix ../../waydroid-fp6-production.nix ];
      }
      ./lomiri-gpu-fp6.nix
      ({ config, lib, ... }: {
        # Same boot image recipe as y2959, only with this generation pinned.
        system.nixos.tags = [ "lomiri-gpu" ];
        assertions = [{
          assertion = config.mobile.boot.stage-1.kernel.package.modDirVersion == kernelRelease
            && config.hardware.fp6Camera.enable
            && config.services.fp6FingerprintPreserved.enable
            && config.services.fp6FingerprintPreserved.mode == "read-only"
            && !config.services.fp6FingerprintPreserved.startAtBoot;
          message = "The Lomiri GPU target must keep the y2959 kernel4/camera/fingerprint settings.";
        }];
      })
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
