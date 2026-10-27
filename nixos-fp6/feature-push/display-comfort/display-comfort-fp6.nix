# Display comfort for Lomiri on the FP6 (feature push 2026-10-24): night light
# (blue-light filter) with schedule, greyscale, dark theme schedule, text and
# display size. Notes, measurements and test results:
# results/feature-push-20261024/display-comfort/README.md.
#
# Night light / greyscale mechanism: a colour filter drawn by the shell.
#  * KMS is not usable on this panel. The msm DPU exposes GAMMA_LUT (1024) and
#    CTM on the CRTC, but they need a DSPP on every layer mixer of the CRTC.
#    The FP6 panel runs dual-DSC with two mixers (LM_2 + LM_3, see debugfs
#    dri/0/state "resource mapping"), and milos has a single DSPP, wired to
#    LM_0 only (catalog/dpu_10_2_milos.h). So the atomic check fails: Mir's
#    own drmModeCrtcSetGamma already logs "Invalid argument" at every Lomiri
#    start. Mir gamma (GammaCurves) or a Mir/qtmir SetGamma API therefore
#    cannot work without a kernel topology change (single DSC/LM), which this
#    feature does not attempt.
#  * FP6.DisplayComfort ColorFilter (qml-plugin/, C++ QSGRenderNode) is the
#    top item of ShellScreen.qml (lomiri-display-comfort.patch): one
#    full-window quad in the shell's render pass. Night light alone =
#    multiplicative blend (dst * white point), which stays in Adreno's tile
#    memory; greyscale = 3x3 matrix via framebuffer fetch or a copy. While
#    neutral there is no scene-graph node at all (no cost).
#    Night light cost (reviewed 2026-10-27, no change): +34 % shell GPU time,
#    but only while the screen animates (120 Hz full-screen load; ~0 when
#    static). This is the fragment work of one more full-screen layer.
#    Qt clears every frame, so freedreno already uses GMEM without the
#    filter, and the blend adds no DRAM pass. The cheaper ideas do not work:
#    there is no extra pass to remove (no FBO/layer/ShaderEffect). Skipping
#    the filter on small-damage frames would flash unfiltered frames, because
#    Qt 5 QSG/qtmir have no partial update and redraw the whole window every
#    frame. Folding the multiply into qtmir's surface material would leave
#    the shell's own Qt Quick items unfiltered. Details: "Night light cost"
#    in the results README. It covers apps,
#    OSK, panel, lock screen. Screenshots (ItemGrabber on the Shell item)
#    keep true colours; screen recordings (frame read-back) include it.
#  * fp6-display-comfort (Rust user service) computes the temperature from
#    the schedule (always / sunset-sunrise via NOAA + GeoClue or fixed
#    coordinates / custom hours, mired ramps) and writes night-light-current;
#    the shell animates each change over 1.5 s.
#  * Greyscale: greyscale-active (the Settings switch, or e.g. bedtime mode).
#
# Dark theme schedule: the daemon switches theme.ini (UITK apps switch live),
# dconf org.gnome.desktop.interface color-scheme / gtk-theme and
# gtk-3.0/settings.ini (GTK, Firefox), optionally com.lomiri.Shell light-mode.
#
# Text and display size (next login; lomiri-session-display-scale.patch):
#  * Display size = the shell's GRID_UNIT_PX (18..32, default 24). qtmir sets
#    the Mir output scale to GU/8 and Mir sends wl_output.scale = ceil(it),
#    so every client (Qt5 UITK, Qt6 Morph, GTK, Firefox with its scale shim)
#    keeps its integer buffer scale and 8 logical px per grid unit and the
#    shell scales the surfaces; 24 = exactly the previous setup.
#  * Text size: UITK_FONT_SCALE (uitk-font-scale.patch: FontUtils and Label
#    pixel sizes), QT_WAYLAND_FORCE_DPI = 96 * ceil(GU/8) * text% (point
#    fonts), GTK text-scaling-factor / gtk-xft-dpi (also Firefox's UI and
#    page scale through GTK's font DPI).
#
# Overlay order: lib.mkOrder 2700, after lomiri-polish (2000), osk-wayland
# (2100), lomiri-multiwindow (2200), lomiri-session (2300), screen-capture
# (2400), drawer-folders (2500), app-ready (2600). The lomiri-session hunk
# replaces only the GRID_UNIT_PX/QT_WAYLAND_FORCE_DPI block of lomiri-session.in
# (as patched by ../../lomiri-session-wayland-grid.patch); lomiri-session-fp6's
# patch touches other files.
{ config, lib, pkgs, options, ... }:
let
  cfg = config.hardware.fairphone6.displayComfort;
  package = pkgs.callPackage ./package.nix { };
  schema = pkgs.callPackage ./schema.nix { };
  qmlPlugin = pkgs.callPackage ./qml-plugin/package.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-display-comfort.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
in {
  options.hardware.fairphone6.displayComfort = {
    enable = lib.mkEnableOption ''
      night light, greyscale, dark theme schedule and text/display size for
      Lomiri (System Settings > Display Comfort)'';
    fontScaleUitk = lib.mkOption {
      type = lib.types.bool;
      # Off for now: it rebuilds lomiri-ui-toolkit and every UITK app. Without
      # it the text size reaches point-sized Qt fonts and GTK/Firefox only.
      default = false;
      description = ''
        Patch lomiri-ui-toolkit so its label sizes follow UITK_FONT_SCALE
        (the Settings text size). Rebuilds every UITK app once.
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      environment.systemPackages = [ package ];
      system.build.fp6-display-comfort = { inherit package schema qmlPlugin lssPlugin; lomiri = pkgs.lomiri.lomiri; };

      nixpkgs.overlays = lib.mkOrder 2700 [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri = lprev.lomiri.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./lomiri-display-comfort.patch ];
            # The colour filter QML module (wrapQtAppsHook adds its QML dir)
            # and the schema (wrapGAppsHook adds its XDG_DATA_DIRS entry).
            buildInputs = (old.buildInputs or [ ]) ++ [ qmlPlugin schema ];
            passthru = (old.passthru or { }) // { fp6DisplayComfort = true; };
          });
          lomiri-session = lprev.lomiri-session.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./lomiri-session-display-scale.patch ];
          });
          lomiri-ui-toolkit = if cfg.fontScaleUitk then lprev.lomiri-ui-toolkit.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./uitk-font-scale.patch ];
          }) else lprev.lomiri-ui-toolkit;
          lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
            plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
          });
        });
      }) ];

      systemd.user.services.fp6-display-comfort = {
        description = "FP6 display comfort (night light / dark theme schedule, scale)";
        wantedBy = [ "graphical-session.target" ];
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        # Not for the LightDM greeter's user manager.
        unitConfig.ConditionUser = user;
        serviceConfig = {
          ExecStart = "${lib.getExe package} run";
          Restart = "on-failure";
          RestartSec = 5;
          Nice = 5;
        };
      };
    }
    # GeoClue may answer the daemon (city accuracy) while Location is on.
    (lib.optionalAttrs (options ? services.fp6Cellular.gnss.allowedApplications) {
      services.fp6Cellular.gnss.allowedApplications = [ "fp6-display-comfort" ];
    })
  ]);
}
