# Build-only test target: the full-features kernel5 Lomiri GPU target plus
# fingerprint-persist-fp6.nix (pmOS-equivalent writable secure storage).
# The writable daemon is MANUAL (no boot autostart); the first start of a boot
# backs up persist + the pmOS SFS tree before mounting persist read-write.
# The read-only preserved module stays installed (manual) because the base
# target asserts it; the two conflict and cannot run together.
# Building never activates anything or writes a phone partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./fingerprint-persist-fp6.nix
      ({ config, lib, ... }: {
        services.fp6FingerprintPersist = {
          enable = true;
          startAtBoot = false;
          # Arms only with a non-empty fingers-10000.map and a running daemon.
          standbyUnlock = true;
          lomiriLockedHint = true;
        };
        system.nixos.tags = [ "fingerprint-persist" ];
        assertions = [ {
          assertion = !config.services.fp6FingerprintPreserved.startAtBoot
            && config.services.fp6FingerprintPreserved.mode == "read-only"
            && config.systemd.services.fp6-fingerprint-persist.wantedBy == [ ];
          message = "The fingerprint persist trial keeps every fingerprint daemon manual.";
        } ];
      })
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
