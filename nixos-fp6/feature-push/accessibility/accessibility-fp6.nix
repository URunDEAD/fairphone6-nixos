# FP6 accessibility (feature push 2026-10-27, roadmap priority 4):
# System Settings > Accessibility with screen magnification, colour inversion,
# colour correction, mono audio and balance; quick-settings tiles for
# inversion and magnification. Notes, previews, tests and the phone checklist:
# results/feature-push-20261024/accessibility/README.md
#
#  * Magnification (qml-plugin/magnifier.*, lomiri-accessibility.patch):
#    a Scale + Translate on ShellScreen's Loader, i.e. on the whole scene
#    (apps are surfaces in the shell's scene graph, so this is the
#    compositor-level transform; Qt Quick maps every touch through it). The
#    colour filter and the "magnified" frame sit outside it. Gestures in an
#    application-level event filter of the shell (sees touches before
#    Lomiri's own filters): N-tap toggle (2-4, default 3), N-tap-and-hold =
#    magnify while held (the finger pans), two-finger pan + pinch while
#    zoomed (one finger still uses the apps; an app gesture in progress gets
#    a TouchCancel), both volume keys held 2 s, and the quick-settings tile.
#    Off (no trigger, not zoomed): no filter, identity transform, no node.
#    With the tap trigger on, taps are held back ~0.3 s (as on Android).
#  * Colour inversion / correction (qml-plugin/colourmath.*): an affine 3x4
#    matrix (Android's hue-preserving inversion, SurfaceFlinger's daltonizer
#    for deuter-/prot-/tritanomaly with strength, greyscale) fed to
#    display-comfort's ColorFilter.preMatrix, so it is folded into the same
#    single full-screen pass as greyscale and night light:
#    night light * greyscale * inversion * correction. Nothing on = identity
#    = no pass at all (night light alone keeps its cheap blend path).
#  * Mono audio / balance (audio/): a WirePlumber script sets an
#    audioconvert filter graph (2x2 mixer) on every stereo output node, so
#    devices, the audio-routing module's selection, links and volumes are
#    untouched; neutral = graph removed. fp6-accessibility-audio (Rust user
#    service) mirrors the GSettings keys into WirePlumber's settings.
#  * GSettings com.fairphone6.accessibility (b/s/d/i keys only).
#
# Overlay at lib.mkOrder 3600: after every other module's Lomiri overlay
# (quick-settings 3100 copies QsBackend.qml in postPatch, lockscreen-polish
# 3200, spread-close-all 3300, mir-upgrade 3500). The patch is applied in
# postPatch on top of all of them; regenerate it with test/mkpatch.sh if the
# context moves (ShellScreen.qml is display-comfort's, Panel.qml/QsBackend.qml
# quick-settings').
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.accessibility;
  schema = pkgs.callPackage ./schema.nix { };
  qmlPlugin = pkgs.callPackage ./qml-plugin/package.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-accessibility.nix { };
  audioBridge = pkgs.callPackage ./fp6-accessibility-audio/package.nix { inherit schema; };
  wireplumberConfig = pkgs.runCommand "fp6-accessibility-wireplumber" { } ''
    install -Dm644 ${./audio/wireplumber-fp6-a11y.conf} $out/share/wireplumber/wireplumber.conf.d/60-fp6-a11y.conf
    install -Dm644 ${./audio/fp6-a11y-audio.lua} $out/share/wireplumber/scripts/fp6/a11y-audio.lua
  '';
  user = config.services.fp6LomiriSensors.user or "user";
in {
  options.hardware.fairphone6.accessibility = {
    enable = lib.mkEnableOption ''
      Settings > Accessibility: screen magnification, colour inversion and
      correction, mono audio and balance (with quick-settings tiles)'';
    audio = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Mono audio and balance (WirePlumber script + fp6-accessibility-audio user service).";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      nixpkgs.overlays = lib.mkOrder 3600 [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri = lprev.lomiri.overrideAttrs (old: {
            # FP6.Accessibility QML module (wrapQtAppsHook adds its QML dir)
            # and the schema (wrapGAppsHook adds its XDG_DATA_DIRS entry).
            buildInputs = (old.buildInputs or [ ]) ++ [ qmlPlugin schema ];
            postPatch = (old.postPatch or "") + ''
              # FP6 accessibility (after every other module's Lomiri edits)
              patch -p1 --no-backup-if-mismatch < ${./lomiri-accessibility.patch}
            '';
            passthru = (old.passthru or { }) // { fp6Accessibility = true; };
          });
          lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
            plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
          });
        });
      }) ];

      environment.systemPackages = [ schema ];

      system.build.fp6-accessibility = {
        inherit schema qmlPlugin lssPlugin audioBridge wireplumberConfig;
        lomiri = pkgs.lomiri.lomiri;
      };

      assertions = [
        { assertion = config.hardware.fairphone6.displayComfort.enable or false;
          message = "accessibility: colour inversion/correction use display-comfort's ColorFilter (preMatrix) and its ShellScreen.qml hunk is the patch context (hardware.fairphone6.displayComfort.enable)."; }
        { assertion = config.hardware.fairphone6.quickSettings.enable or false;
          message = "accessibility: the Inversion/Magnification tiles patch quick-settings' QsBackend.qml/Panel.qml (hardware.fairphone6.quickSettings.enable)."; }
      ];
    }
    (lib.mkIf cfg.audio {
      services.pipewire.wireplumber.configPackages = [ wireplumberConfig ];

      systemd.user.services.fp6-accessibility-audio = {
        description = "FP6 accessibility: mono audio / balance -> WirePlumber";
        wantedBy = [ "graphical-session.target" ];
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" "wireplumber.service" ];
        # Not for the LightDM greeter's user manager.
        unitConfig.ConditionUser = user;
        serviceConfig = {
          ExecStart = "${lib.getExe audioBridge} run";
          Restart = "on-failure";
          RestartSec = 5;
          Nice = 10;
        };
      };
    })
  ]);
}
