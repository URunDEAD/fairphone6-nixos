# Morph Browser horizontal-squeeze fix for the Lomiri GPU session. UNVERIFIED
# on hardware (Morph was not running during the read-only inspection).
#
# Diagnosis (source-level, pinned nixpkgs 26.11pre1031299):
#  * lomiri-session publishes QT_WAYLAND_FORCE_DPI=288 (12 * GRID_UNIT_PX 24)
#    and GRID_UNIT_PX=8 to the systemd user environment (seen on the phone:
#    `systemctl --user show-environment`). Mir announces wl_output scale 3.
#  * Qt5 apps (the rest of the UT app set) ignore logical DPI for scaling
#    (QT_AUTO_SCREEN_SCALE_FACTOR=0), so they only use the Wayland DPR 3.
#  * Morph is the only Qt6 app (pkgs.lomiri-qt6.morph-browser). Qt 6.11
#    always enables high-DPI scaling: QHighDpiScaling::rawScaleFactor() =
#    QWaylandScreen::logicalDpi() / 96 = 288 / 96 = 3, and that factor is
#    multiplied with the Wayland buffer scale 3 -> effective DPR 9. The
#    window is then only 1116 / 9 = 124 device-independent px wide, while
#    Morph's BrowserWindow has minimumWidth: units.gu(40) = 320. Morph commits
#    a surface 320 * 3 = 960 logical px wide instead of the 372 Lomiri asked for.
#  * fp6's lomiri-wayland-surface-scale.patch shows client surfaces with
#    MirSurfaceItem.Stretch into the requested width: 960 -> 372 is a 2.6x
#    horizontal squeeze. The vertical size matches, so text looks condensed.
#
# Fix: start Morph (and its webapp-container) without QT_WAYLAND_FORCE_DPI, so
# Qt6 falls back to 96 dpi (Qt factor 1) and only the Wayland scale 3 applies,
# the same as the Qt5 apps. GRID_UNIT_PX=8 then gives 8 logical = 24 physical
# px per grid unit. The shell and the Qt5 apps are unchanged. The original
# package is wrapped with symlinkJoin, so nothing is rebuilt.
#
# How to check on the phone: tr '\0' '\n' < /proc/$(pgrep -f bin/.morph-browser-wrapped)/environ
# should show no QT_WAYLAND_FORCE_DPI, and the chrome must fill the width
# without being squeezed. If it still squeezes, try `extraEnv = { QT_ENABLE_HIGHDPI_SCALING = "0"; }`.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriGpu.morphDpiFix;
  envArgs = lib.concatStringsSep " " (
    map (v: "--unset ${v}") cfg.unsetEnv
    ++ lib.mapAttrsToList (k: v: "--set ${k} ${lib.escapeShellArg v}") cfg.extraEnv);
  wrap = orig: pkgs.symlinkJoin {
    name = "${orig.pname or "morph-browser"}-fp6-dpi-${orig.version or "0"}";
    paths = [ orig ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      for prog in morph-browser webapp-container; do
        if [ -e "$out/bin/$prog" ]; then
          rm "$out/bin/$prog"
          makeWrapper ${orig}/bin/$prog "$out/bin/$prog" ${envArgs}
        fi
      done
      # The desktop files use Exec=morph-browser (PATH lookup); pin them to
      # the wrapper so a launcher with a different PATH order still uses it.
      for f in "$out"/share/applications/*.desktop; do
        [ -e "$f" ] || continue
        target=$(readlink -f "$f")
        rm "$f"
        sed -e "s|^Exec=morph-browser|Exec=$out/bin/morph-browser|" \
            -e "s|^Exec=webapp-container|Exec=$out/bin/webapp-container|" "$target" > "$f"
      done
    '';
    passthru = (orig.passthru or { }) // { fp6Unwrapped = orig; };
    inherit (orig) meta;
  };
in {
  options.hardware.fairphone6.lomiriGpu.morphDpiFix = {
    enable = lib.mkEnableOption ''
      starting the Qt6 Morph Browser without QT_WAYLAND_FORCE_DPI, so Qt6's
      DPI scale does not stack on the Wayland output scale (UNVERIFIED fix for
      the horizontally squeezed Morph UI)'';
    unsetEnv = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "QT_WAYLAND_FORCE_DPI" "QT_SCALE_FACTOR" "QT_SCREEN_SCALE_FACTORS" ];
      description = "Variables removed from Morph's environment.";
    };
    extraEnv = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = { QT_ENABLE_HIGHDPI_SCALING = "0"; };
      description = "Extra variables set for Morph only.";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = [ (final: prev: {
      lomiri-qt6 = prev.lomiri-qt6.overrideScope (_lfinal: lprev: {
        morph-browser = wrap lprev.morph-browser;
      });
    }) ];
    system.build.fp6-morph-browser = pkgs.lomiri-qt6.morph-browser;
  };
}
