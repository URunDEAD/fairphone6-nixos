{ lib, ... }:
{
  nixpkgs.overlays = lib.mkAfter [ (import ./apparmor-runtime-helper-overlay.nix) ];
}
