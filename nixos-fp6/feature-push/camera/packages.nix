# Standalone build entry (no system, no activation):
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file nixos-fp6/feature-push/camera/packages.nix --no-link --print-out-paths
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  # Same derivation the running y2959 generation uses (hardware.fp6Camera.package).
  libcamera = pkgs.callPackage ../../selfie-fp6-libcamera.nix { };
  app = pkgs.callPackage ./lomiri-camera-app-fp6.nix { inherit libcamera; };
  # The exact derivation the Lomiri camera trial system installs (its nixpkgs
  # carries the Lomiri/GPU agent's qtdeclarative, so it differs from `app`).
  trial = import ./trial-lomiri-camera-fp6.nix { inherit nixpkgs; };
in {
  inherit libcamera;
  system-lomiri-camera-app = trial.config.system.build.fp6-lomiri-camera-app;
  gst-fp6camera = app.gstFp6Camera;
  qtmultimedia-fp6 = app.qtmultimediaFp6;
  lomiri-camera-app-fp6 = app;
}
