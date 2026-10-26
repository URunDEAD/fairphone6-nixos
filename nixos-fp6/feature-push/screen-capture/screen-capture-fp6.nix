# Screenshots and screen recording for Lomiri on the FP6.
# Notes and live results: results/feature-push-20261024/screen-capture/README.md.
#
# Screenshots (Lomiri's own ItemGrabber, grabs the shell's scene graph, so
# panel/launcher/apps and the current orientation are in the picture):
#  * Power + Volume Down (either order; Lomiri also keeps its Volume Up +
#    Volume Down chord) or "Screenshot" in the power-key dialog (hold Power).
#    fp6-lomiri-powerd (sensors/) leaves the display on for a power press that
#    is part of that chord. PNG in ~/Pictures/Screenshots/, white flash and
#    the camera shutter sound (its path is fixed for NixOS here).
#
# Screen recording (new; Lomiri/Ubuntu Touch have none):
#  * Capture: Mir's wlr-screencopy only sees Mir's own scene (the app
#    surfaces at Mir positions) and returns black on this Lomiri, because the
#    shell is composited by Qt. So the shell itself reads back what it has
#    rendered: the FP6.ScreenRecorder QML item (qml-plugin/, C++) blits each
#    rendered frame (scaled to `recordWidth`, at most `recordFps`, PBO
#    read-back without GPU stalls) and passes it over a socketpair to its
#    own child fp6-screenrecord (recorder/, Rust + GStreamer): Iris V4L2
#    H.264 encoder (x264 fallback), optional microphone and/or phone sound
#    (PipeWire/Pulse monitor) as AAC, MP4 in ~/Videos/Screencasts/.
#  * Privacy: frames only flow while the shell is unlocked and the display
#    is on (the item's `allowed` binding; locking, the greeter or display off
#    stop the recording before another frame is read). There is no socket,
#    D-Bus method or Wayland global that hands out pixels, and every start
#    needs a tap on "Start recording" in the shell's own dialog. qtmir enables
#    zwlr_screencopy_manager_v1 for every client; it shows nothing useful on
#    this shell, and this module does not touch it.
#  * UI: "Record screen" in the power-key dialog, or the "Record screen"
#    switch in the status-bar menus (fp6-screenrecord-indicator, Rust): both
#    open the confirmation (microphone / phone sound switches), then a 3 s
#    countdown. While recording a pulsing red dot sits left of the panel
#    title; tap it (or "Stop recording" in the power dialog, or the switch)
#    to stop. A notification says where the video was saved.
#
# Lomiri patch (lomiri-screen-capture.patch, QML only): PhysicalKeysMapper
# (chord), ItemGrabber (sound path placeholder), Shell.qml (recorder item,
# countdown), Dialogs.qml (power dialog entry + confirmation), Panel.qml
# (red dot). Overlay lib.mkOrder 2400, after lomiri-polish (2000),
# osk-wayland (2100), lomiri-multiwindow (2200) and lomiri-session (2300).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.screenCapture;
  recorder = pkgs.callPackage ./recorder/package.nix { };
  qmlPlugin = pkgs.callPackage ./qml-plugin/package.nix {
    inherit recorder;
    recordWidth = cfg.recordWidth;
    recordFps = cfg.recordFps;
  };
  indicator = pkgs.runCommand "fp6-screenrecord-indicator" {
    meta.mainProgram = "fp6-screenrecord-indicator";
  } ''
    mkdir -p $out/bin $out/share/ayatana/indicators
    ln -s ${recorder}/bin/fp6-screenrecord-indicator $out/bin/
    cat > $out/share/ayatana/indicators/com.lomiri.indicator.screenrecord <<EOF
    [Indicator Service]
    Name=fp6-screenrecord-indicator
    ObjectPath=/com/lomiri/indicator/screenrecord
    Position=55

    [phone]
    ObjectPath=/com/lomiri/indicator/screenrecord/phone
    Position=55

    [desktop]
    ObjectPath=/com/lomiri/indicator/screenrecord/desktop
    EOF
  '';
in {
  options.hardware.fairphone6.screenCapture = {
    enable = lib.mkEnableOption ''
      screenshots (Power + Volume Down, shutter sound) and a Lomiri screen
      recorder (power dialog / status-bar switch, red dot, MP4 in ~/Videos/Screencasts)'';
    recordWidth = lib.mkOption {
      type = lib.types.ints.between 240 1116;
      default = 720;
      description = "Width of the recorded video in portrait (height follows the screen, 1116x2484 -> 720x1602).";
    };
    recordFps = lib.mkOption {
      type = lib.types.ints.between 5 60;
      default = 30;
      description = "Maximum frame rate of a recording (a still screen produces no new frames).";
    };
    indicator = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Status-bar menu switch \"Record screen\" (shown in the expanded indicator menus only).";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 2400 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-screen-capture.patch ];
          postPatch = (old.postPatch or "") + ''
            substituteInPlace qml/Components/ItemGrabber.qml \
              --replace-fail '@fp6CameraClick@' \
                '${lprev.lomiri-sounds}/share/sounds/lomiri/camera/click/camera_click.ogg'
          '';
          # wrapQtAppsHook adds its QML directory to the lomiri wrapper.
          buildInputs = (old.buildInputs or [ ]) ++ [ qmlPlugin ];
          passthru = (old.passthru or { }) // { fp6ScreenCapture = true; };
        });
      });
    }) ];

    environment.systemPackages = lib.optional cfg.indicator indicator;
    environment.pathsToLink = lib.optional cfg.indicator "/share/ayatana";
    systemd.user.services.fp6-screenrecord-indicator = lib.mkIf cfg.indicator {
      description = "Screen recording switch for Lomiri's status-bar menus";
      partOf = [ "graphical-session.target" "ayatana-indicators.target" "lomiri-indicators.target" ];
      wantedBy = [ "ayatana-indicators.target" "lomiri-indicators.target" ];
      serviceConfig = {
        ExecStart = lib.getExe indicator;
        Restart = "on-failure";
        RestartSec = 5;
      };
    };

    system.build.fp6-screen-capture = { inherit recorder qmlPlugin indicator; lomiri = pkgs.lomiri.lomiri; };
  };
}
