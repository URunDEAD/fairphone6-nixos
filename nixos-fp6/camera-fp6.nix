# Opt-in staging only. Not imported by the phone's default or trial systems.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fp6Camera;
  cameraPipeWire = pkgs.callPackage ./camera-fp6-pipewire.nix { libcamera = cfg.package; };
in {
  options.hardware.fp6Camera = {
    enable = lib.mkEnableOption "staged FP6 rear-camera userspace diagnostics";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./camera-fp6-pmos-libcamera.nix { };
      description = "Original FP6 libcamera software-ISP/AF/tuning; live capture is unvalidated.";
    };
    enablePipeWire = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Build PipeWire's libcamera SPA plugin against the same staged camera package.";
    };
    enableSnapshot = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Install Snapshot's native PipeWire camera UI for later authorized testing.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !cfg.enableSnapshot || cfg.enablePipeWire;
        message = "FP6 Snapshot requires the matching PipeWire camera backend.";
      }
      {
        assertion = !cfg.enablePipeWire ||
          (config.services.pipewire.enable && config.services.pipewire.wireplumber.enable);
        message = "FP6 camera integration requires PipeWire and WirePlumber services.";
      }
    ];
    # Snapshot's wrapper embeds its PipeWire device-provider and libcamera
    # plugin paths. Keep every app-list reference on the matching camera stack.
    nixpkgs.overlays = lib.optional cfg.enableSnapshot (final: prev: {
      snapshot = prev.snapshot.override {
        libcamera = cfg.package;
        pipewire = cameraPipeWire;
      };
    });
    environment.systemPackages = [ cfg.package pkgs.v4l-utils pkgs.gst_all_1.gstreamer ]
      ++ lib.optional cfg.enableSnapshot pkgs.snapshot;
    services.pipewire.package = lib.mkIf cfg.enablePipeWire
      (lib.mkDefault cameraPipeWire);
    # WirePlumber loads the libcamera SPA monitor in its own process. Rebuild
    # against the same PipeWire so its compiled-in SPA path cannot stay stock.
    services.pipewire.wireplumber.package = lib.mkIf cfg.enablePipeWire
      (lib.mkDefault (pkgs.wireplumber.override { pipewire = cameraPipeWire; }));
  };
}
