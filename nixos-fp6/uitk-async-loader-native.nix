{ lib, ... }:
{
  nixpkgs.overlays = lib.mkAfter [ (import ./uitk-async-loader-overlay.nix) ];
}
