{ pkgs ? import ../../upstream/mobile-nixos/npins }:
let p = if pkgs ? nixpkgs then import pkgs.nixpkgs {} else pkgs;
in p.mkShell {
  packages = with p; [ cargo rustc rustfmt clippy pkg-config libpulseaudio dbus python3 ];
  shellHook = ''export CARGO_HOME=''${CARGO_HOME:-$PWD/.cargo-home}
    export CARGO_TARGET_DIR=''${CARGO_TARGET_DIR:-$PWD/target}'';
}
