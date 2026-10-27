{ pkgs ? import <nixpkgs> {} }:
pkgs.callPackage ./package.nix {
  lomiriUiToolkit = import ./nix/qt5-toolkit.nix { inherit pkgs; };
  suruIconTheme = pkgs.lomiri.suru-icon-theme;
}
