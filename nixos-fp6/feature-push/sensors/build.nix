# Userspace artifacts for the sensors feature push (aarch64, pinned nixpkgs).
#   .macmini-builder/nix-remote build --eval-store daemon --file nixos-fp6/feature-push/sensors/build.nix \
#     -A fp6-lomiri-powerd --no-link --print-out-paths
#   ... -A qtmir-orientation   (production qtmir overlay + orientation owner-thread fix)
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  production = (import ../../default.nix { inherit nixpkgs; }).pkgs;
in {
  fp6-lomiri-powerd = pkgs.callPackage ./package.nix { };
  qtmir-orientation = production.lomiri.qtmir.overrideAttrs (old: {
    patches = (old.patches or []) ++ [ ../../qtmir-orientation-owner-thread.patch ];
  });
}
