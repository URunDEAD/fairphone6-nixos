{ pkgs ? import ((import ../upstream/mobile-nixos/npins).nixpkgs) { system = "aarch64-linux"; }
, binaryArchive ? null
}:
let
  source = pkgs.callPackage ./source.nix {};
  runtime = pkgs.callPackage ./runtime.nix { sourcePackage = source; };
in {
  binary = assert binaryArchive != null; pkgs.callPackage ./binary.nix { archive = binaryArchive; };
  inherit source runtime;
  sourceConfigure = pkgs.callPackage ./source.nix { configureOnly = true; };
  tools = pkgs.callPackage ./tools.nix { overlay = runtime; };
}
