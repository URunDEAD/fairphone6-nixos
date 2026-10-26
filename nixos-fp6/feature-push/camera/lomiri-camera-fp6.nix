# FP6 photo/video/switching/flash for lomiri-camera-app (Lomiri + Mir).
#
# Path: lomiri-camera-app (Qt5 QML Camera) -> QtMultimedia gstreamer camerabin
#   (rebuilt with GstPhotography + FP6 patch) -> fp6camerabinsrc -> fp6libcamerasrc
#   -> libcamerasrc (hardware.fp6Camera.package: pmOS softISP/AF/tuning + selfie
#   helper) -> CAMSS RDI.  Flash = white:flash torch brightness, bounded.
#
# Opt-in; nothing here loads modules, starts services or touches the cameras at
# boot. Requires the existing camera stack (hardware.fp6Camera.enable).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fp6LomiriCamera;
  camOpts = { name, ... }: {
    options = {
      cameraName = lib.mkOption { type = lib.types.str; description = "libcamera camera id"; };
      width = lib.mkOption { type = lib.types.ints.positive; };
      height = lib.mkOption { type = lib.types.ints.positive; };
      sizes = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        description = "WxH sizes offered to the app (never above width x height; keep the stream aspect).";
      };
      mountAngle = lib.mkOption {
        type = lib.types.enum [ 0 90 180 270 ];
        description = "Android-style sensor orientation (clockwise) as QCameraInfo::orientation; = (360 - libcamera Rotation / DT rotation) % 360.";
      };
      flip = lib.mkOption {
        type = lib.types.enum [ "none" "horizontal-flip" "vertical-flip" "rotate-180" ];
        default = "none";
        description = "videoflip method applied in sensor coordinates (before the app's rotation by mountAngle); fixes sensors that read out mirrored.";
      };
      swapRB = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Relabel the stream with red and blue exchanged (sensor Bayer order reported off by one).";
      };
      afMode = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "Optional libcamerasrc af-mode value (e.g. continuous); empty leaves libcamera's default.";
      };
    };
  };
  camSection = n: c: ''
    [${n}]
    camera-name=${c.cameraName}
    width=${toString c.width}
    height=${toString c.height}
    format=BGRx
    sizes=${lib.concatStringsSep ";" c.sizes}
    mount-angle=${toString c.mountAngle}
    af-mode=${c.afMode}
    flip=${c.flip}
    swap-rb=${lib.boolToString c.swapRB}
  '';
  confText = camSection "primary" cfg.primary + camSection "secondary" cfg.secondary
    + camSection "ultrawide" cfg.ultrawide + ''
    [flash]
    led-brightness-path=/sys/class/leds/white:flash/brightness
    brightness=${toString cfg.flash.brightness}
    preflash-ms=${toString cfg.flash.preflashMs}
    capture-hold-max-ms=${toString cfg.flash.captureHoldMaxMs}
    video-light-max-s=${toString cfg.flash.videoLightMaxSeconds}
    auto-is-on=${lib.boolToString cfg.flash.autoIsOn}
  '';
in {
  imports = [ ../../torch-fp6.nix ];

  options.hardware.fp6LomiriCamera = {
    enable = lib.mkEnableOption "lomiri-camera-app on the FP6 libcamera stack";
    frontBayerFixed = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        The fp6-s5kkd1 driver reports the correct Bayer order (GBRG), so the
        front stream needs no swap-rb relabel. Set by selfie-gbrg-fp6.nix.
      '';
    };
    hardwareEncode = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Allow encodebin to pick the Iris V4L2 m2m H.264 encoder (v4l2h264enc).
        Off by default (x264enc ultrafast) until tools/feature-push/camera-test.sh venc passes.
      '';
    };
    primary = lib.mkOption {
      type = lib.types.submodule camOpts;
      description = "Back camera (Qt 'primary').";
      default = {
        cameraName = "/base/soc@0/cci@ac15000/i2c-bus@0/camera@1a";   # IMX896 main
        width = 2048; height = 1536;
        sizes = [ "2048x1536" "1440x1080" "960x720" "640x480" ];
        # DT rotation = 90 is libcamera's Rotation (anticlockwise); Qt/lomiri
        # expect the Android orientation (clockwise) = (360 - 90) % 360.
        # 90 showed the viewfinder upside down (2026-10-26).
        mountAngle = 270;
        # The left-right mirror the user also saw was the app's own
        # "desktop mode" viewfinder mirror (fixed by
        # lomiri-camera-app-wayland-device-mode.patch), not the sensor:
        # with it gone, photos and the viewfinder agree, so no flip here.
      };
    };
    secondary = lib.mkOption {
      type = lib.types.submodule camOpts;
      description = "Front camera (Qt 'secondary').";
      default = {
        cameraName = "/base/soc@0/cci@ac16000/i2c-bus@1/camera@3d";   # S5KKD1 selfie
        width = 1536; height = 1152;
        sizes = [ "1536x1152" "1440x1080" "960x720" "640x480" ];
        mountAngle = 270;           # vendor roll 270; NOT physically verified yet
        # Selfies came out with blue skin: R and B exchanged (2026-10-26;
        # swapping the channels of a saved selfie gave natural skin). Fixed
        # at the source by the GBRG fp6-s5kkd1 driver (selfie-gbrg-fp6.nix,
        # sets frontBayerFixed); the relabel stays only for older drivers.
        swapRB = !cfg.frontBayerFixed;
      };
    };
    ultrawide = lib.mkOption {
      type = lib.types.submodule camOpts;
      description = "Back ultrawide camera (the app's 0.6x lens button; replaces 'primary' while selected).";
      default = {
        cameraName = "/base/soc@0/cci@ac15000/i2c-bus@1/camera@36";   # OV13B10 ultrawide
        width = 2048; height = 1536;
        sizes = [ "2048x1536" "1440x1080" "960x720" "640x480" ];
        mountAngle = 270;
        # Mounted 180 degrees from the main sensor (frame checked
        # 2026-10-26); Qt keeps the primary's orientation, so turn the stream.
        flip = "rotate-180";
      };
    };
    flash = {
      brightness = lib.mkOption { type = lib.types.ints.between 1 255; default = 160; };
      preflashMs = lib.mkOption { type = lib.types.ints.between 0 1500; default = 400; };
      captureHoldMaxMs = lib.mkOption { type = lib.types.ints.between 100 3000; default = 1100; };
      videoLightMaxSeconds = lib.mkOption { type = lib.types.ints.between 1 3600; default = 600; };
      autoIsOn = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "No light metering exists; Auto behaves as Off unless this is set.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.hardware.fp6Camera.enable;
      message = "hardware.fp6LomiriCamera needs hardware.fp6Camera (libcamera package/IPA).";
    }];
    # LED brightness write access for the video group (same rule as the torch).
    hardware.fp6Torch.enable = true;
    environment.etc."fp6-camera/camera.conf".text = confText;
    nixpkgs.overlays = [
      (final: prev: {
        lomiri = prev.lomiri.overrideScope (lfinal: lprev: {
          lomiri-camera-app = final.callPackage ./lomiri-camera-app-fp6.nix {
            lomiri = prev.lomiri;
            libcamera = config.hardware.fp6Camera.package;
            inherit (cfg) hardwareEncode;
          };
        });
      })
    ];
    environment.systemPackages = [ pkgs.gst_all_1.gstreamer ];
    system.build.fp6-lomiri-camera-app = pkgs.lomiri.lomiri-camera-app;
  };
}
