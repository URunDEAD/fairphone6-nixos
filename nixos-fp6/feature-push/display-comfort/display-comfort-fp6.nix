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
#    pixel sizes; with fontScaleUitk a patched libLomiriToolkit.so.5 is put
#    first on LD_LIBRARY_PATH instead of rebuilding ~50 UITK dependants),
#    QT_WAYLAND_FORCE_DPI = 96 * ceil(GU/8) * text% (point
#    fonts), GTK text-scaling-factor / gtk-xft-dpi (also Firefox's UI and
#    page scale through GTK's font DPI).
#    Live: the daemon writes ~/.config/lomiri-ui-toolkit/font-scale, which
#    the font-scale UITK watches (re-emits gridUnitChanged, so running apps
#    and the shell relayout), and the GTK scale on every change. Layouts that
#    broke at 130-150 % are patched to grow with the text: Settings main grid
#    (lss-text-size.patch), app drawer (lomiri-text-size.patch), Messages
#    thread rows (messaging-text-size.patch via messagingPatches).
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
  # UITK font scale without rebuilding UITK's dependants: the system's own
  # UITK (all overlays applied) plus uitk-font-scale.patch, built once. Only
  # libLomiriToolkit.so.5 is exposed. Every UITK consumer (QML plugin
  # libLomiriComponents.so, lomiri, Settings, ...) finds it through RUNPATH
  # (--enable-new-dtags), which LD_LIBRARY_PATH precedes; glibc then reuses
  # the loaded object by soname. The patch adds only a static function, so
  # the ABI and the QML files of the stock UITK stay valid. AppArmor: the FP6
  # profiles attach from unconfined and exec with ix, so no AT_SECURE scrub.
  uitkFontScale = pkgs.lomiri.lomiri-ui-toolkit.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./uitk-font-scale.patch ];
  });
  uitkFontScaleLib = pkgs.runCommand "fp6-uitk-font-scale-lib" { } ''
    mkdir -p $out/lib
    ln -s ${uitkFontScale}/lib/libLomiriToolkit.so.5 $out/lib/libLomiriToolkit.so.5
    test -e $out/lib/libLomiriToolkit.so.5
  '';
  # Offscreen pixel-size check of the above (build it to run it).
  uitkFontScaleTest = pkgs.callPackage ./uitk-font-scale-test { inherit uitkFontScaleLib; };
in {
  options.hardware.fairphone6.displayComfort = {
    messagingPatches = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      internal = true;
      description = ''
        Patches for lomiri-messaging-app (the thread list rows grow with the
        text size). Applied by contacts/contacts-fp6.nix, which builds the
        app.
      '';
    };
    enable = lib.mkEnableOption ''
      night light, greyscale, dark theme schedule and text/display size for
      Lomiri (System Settings > Display Comfort)'';
    fontScaleUitk = lib.mkOption {
      type = lib.types.bool;
      # On: one extra UITK build, no app rebuilds; offscreen-checked by
      # uitkFontScaleTest (1.00 = pixel-identical to stock).
      default = true;
      description = ''
        Let UITK label sizes (shell and Lomiri apps) follow UITK_FONT_SCALE
        (the Settings text size). A libLomiriToolkit.so.5 built with
        uitk-font-scale.patch is put first on LD_LIBRARY_PATH by
        lomiri-session; the stock UITK and every app keep their store paths,
        so nothing else is rebuilt.
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      hardware.fairphone6.displayComfort.messagingPatches = [ ./messaging-text-size.patch ];
      environment.systemPackages = [ package ];
      system.build.fp6-display-comfort = { inherit package schema qmlPlugin lssPlugin uitkFontScale uitkFontScaleLib uitkFontScaleTest; lomiri = pkgs.lomiri.lomiri; };

      nixpkgs.overlays = lib.mkOrder 2700 [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri = lprev.lomiri.overrideAttrs (old: {
            # lomiri-text-size.patch (app drawer cells grow with large
            # text) is written against the drawer-folders Drawer.qml.
            patches = (old.patches or [ ]) ++ [ ./lomiri-display-comfort.patch ]
              ++ lib.optional (config.hardware.fairphone6.lomiriDrawerFolders.enable or false) ./lomiri-text-size.patch;
            # The colour filter QML module (wrapQtAppsHook adds its QML dir)
            # and the schema (wrapGAppsHook adds its XDG_DATA_DIRS entry).
            buildInputs = (old.buildInputs or [ ]) ++ [ qmlPlugin schema ];
            passthru = (old.passthru or { }) // { fp6DisplayComfort = true; };
          });
          lomiri-session = lprev.lomiri-session.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./lomiri-session-display-scale.patch ];
            # The shell (exec'd below) and, through the systemd/D-Bus
            # activation environment, every app load the font-scale UITK.
            # Set at every login whatever the scale (1.00 = stock sizes), so
            # no stale value can survive in a running user manager.
            postPatch = (old.postPatch or "") + lib.optionalString cfg.fontScaleUitk ''
              substituteInPlace lomiri-session.in --replace-fail \
                'dbus-update-activation-environment --systemd UITK_FONT_SCALE=''${UITK_FONT_SCALE}' \
                'dbus-update-activation-environment --systemd UITK_FONT_SCALE=''${UITK_FONT_SCALE}
                export LD_LIBRARY_PATH=${uitkFontScaleLib}/lib''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
                dbus-update-activation-environment --systemd LD_LIBRARY_PATH="$LD_LIBRARY_PATH"'
            '';
          });
          # Main page grid: cells widen with the text size (no mid-word
          # breaks at 150 %) and relayout on a live text size change.
          lomiri-system-settings-unwrapped = lprev.lomiri-system-settings-unwrapped.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./lss-text-size.patch ];
          });
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
