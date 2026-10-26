{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs }:
let mobile = import ./upstream/mobile-nixos {
  pkgs = import nixpkgs { system = "aarch64-linux"; config.allowUnfree = true; };
  device = ./device;
  configuration = ./configuration.nix;
};
in mobile // {
  portClosure = mobile.pkgs.closureInfo { rootPaths = [ mobile.config.system.build.toplevel ]; };
}
