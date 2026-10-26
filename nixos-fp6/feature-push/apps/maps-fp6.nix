# Pure Maps (nixpkgs pure-maps 3.4.2, libsForQt5) as the maps app of the
# Lomiri GPU session. See results/feature-push-20261024/maps/README.md.
#
#  * Flavor (tested in the private display, 2026-10-26): nixpkgs builds
#    -DFLAVOR=qtcontrols. On this pin it shows a blank window (no
#    QtGraphicalEffects in its closure), and with it added the UI sizes come
#    from the application font, which Lomiri's QT_WAYLAND_FORCE_DPI=288 makes
#    ~4x too big (list rows cut to "OpenFre..."). "uuitk" (default) builds
#    the Ubuntu Touch flavour: Lomiri.Components units/theme (GRID_UNIT_PX
#    like the rest of the UT app set), Suru QQC2 style, QtSystemInfo
#    screensaver inhibit, Lomiri.Content file picker. Its CMake points the
#    data dir at "./share/pure-maps" (click-relative); patched to the
#    absolute install path.
#  * Providers: the nixpkgs build ships no API keys (poor/apikeys.py empty).
#    Keyless and default: OpenFreeMap vector tiles, Photon search. The
#    upstream default router (Stadia Maps) needs a key, so the default
#    router is set to OSRM (router.project-osrm.org, car only). Foursquare
#    (nearby/guide) needs a key; OSM Scout (offline) needs OSM Scout Server.
#  * Position: QtPositioning's geoclue2 plugin (qtlocation) is on the
#    wrapper's QT_PLUGIN_PATH. The client's GeoClue desktop id is pinned to
#    the .desktop id with QT_GEOCLUE_APP_DESKTOP_ID. GeoClue asks the
#    user's agent (fp6-location-indicator), which authorizes any app while
#    the location switch is on; no geoclue.conf entry is needed.
#  * Launcher: the desktop entry gets an absolute Exec, X-Lomiri-Touch=true
#    (the drawer lists only those) and an absolute icon PNG file copied into
#    the wrapper (not a symlink).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.maps;
  uuitk = cfg.flavor == "uuitk";
  desktopId = "io.github.rinigus.PureMaps";

  unwrapped = pkgs.pure-maps.overrideAttrs (old: {
    pname = "pure-maps-fp6-${cfg.flavor}";
    cmakeFlags = (lib.filter (f: !lib.hasPrefix "-DFLAVOR=" f) (old.cmakeFlags or [ ])) ++ [
      "-DFLAVOR=${cfg.flavor}"
    ];
    # cmakeFlags is split on whitespace ("OpenFreeMap Bright" would become
    # "OpenFreeMap" -> unknown provider -> OpenTopoMap fallback); the array
    # keeps each value in one argument.
    preConfigure = (old.preConfigure or "") + ''
      cmakeFlagsArray+=(
        ${lib.escapeShellArg "-DDEFAULT_ROUTER=${cfg.defaultRouter}"}
        ${lib.escapeShellArg "-DDEFAULT_GEOCODER=${cfg.defaultGeocoder}"}
        ${lib.escapeShellArg "-DDEFAULT_BASEMAP=${cfg.defaultBasemap}"}
      )
    '';
    buildInputs = old.buildInputs ++ lib.optionals uuitk [
      pkgs.lomiri.lomiri-ui-toolkit
      pkgs.lomiri.lomiri-content-hub
      pkgs.lomiri.qqc2-suru-style
      pkgs.libsForQt5.qtsystems
      pkgs.libsForQt5.qtgraphicaleffects
    ] ++ lib.optionals (!uuitk) [
      # nixpkgs' qtcontrols build misses it: BasemapButton.qml fails
      # ("module QtGraphicalEffects is not installed") -> blank window.
      pkgs.libsForQt5.qtgraphicaleffects
    ];
    postInstall = (old.postInstall or "") + ''
      c=$out/share/pure-maps/poor/config.py
      grep -qF ${lib.escapeShellArg "_default_basemap = \"${cfg.defaultBasemap}\""} $c
      grep -qF ${lib.escapeShellArg "_default_router = \"${cfg.defaultRouter}\""} $c
      grep -qF ${lib.escapeShellArg "_default_geocoder = \"${cfg.defaultGeocoder}\""} $c
    '';
    postPatch = (old.postPatch or "") + lib.optionalString uuitk ''
      substituteInPlace CMakeLists.txt \
        --replace-fail 'set(DATADIR_RUNNING "./share/''${APP_NAME}")' 'set(DATADIR_RUNNING ''${DATADIR})'
    '';
  });

  envArgs = lib.concatStringsSep " " (
    [ "--set QT_GEOCLUE_APP_DESKTOP_ID ${desktopId}" ]
    ++ lib.mapAttrsToList (k: v: "--set ${k} ${lib.escapeShellArg v}") cfg.extraEnv
    ++ map (v: "--unset ${v}") cfg.unsetEnv);

  pureMaps = pkgs.symlinkJoin {
    name = "pure-maps-lomiri-fp6-${unwrapped.version}";
    paths = [ unwrapped ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      rm "$out/bin/pure-maps"
      makeWrapper ${unwrapped}/bin/pure-maps "$out/bin/pure-maps" ${envArgs}
      # Real PNG file (not a store symlink) for the launcher icon.
      rm -f "$out/share/pixmaps/${desktopId}.png"
      install -Dm644 ${unwrapped}/share/icons/hicolor/256x256/apps/${desktopId}.png \
        "$out/share/pixmaps/${desktopId}.png"
      for f in "$out"/share/applications/*.desktop; do
        src=$(readlink -f "$f"); rm "$f"
        sed -e "s|^Exec=[^ ]*pure-maps|Exec=$out/bin/pure-maps|" \
            -e '/^X-Ubuntu-Touch=/d' -e '/^X-Lomiri-Touch=/d' "$src" > "$f"
      done
      d="$out/share/applications/${desktopId}.desktop"
      sed -i -e "s|^Icon=.*|Icon=$out/share/pixmaps/${desktopId}.png|" \
             -e '/^\[Desktop Entry\]$/a X-Lomiri-Touch=true' "$d"
      # The URI handler entry is not a launcher.
      grep -q '^NoDisplay=' "$out/share/applications/pure-maps-uri-handler.desktop" \
        || sed -i '/^\[Desktop Entry\]$/a NoDisplay=true' "$out/share/applications/pure-maps-uri-handler.desktop"
      grep -q "^Exec=$out/bin/pure-maps" "$d"
      test "$(grep -c '^X-Lomiri-Touch=true$' "$d")" = 1
    '';
    passthru = { inherit unwrapped desktopId; };
    meta = unwrapped.meta;
  };
in {
  options.hardware.fairphone6.maps = {
    enable = lib.mkEnableOption "Pure Maps wrapped for Lomiri/Mir";
    flavor = lib.mkOption {
      type = lib.types.enum [ "qtcontrols" "kirigami" "uuitk" ];
      default = "uuitk";
      description = "Pure Maps UI platform (nixpkgs uses qtcontrols).";
    };
    defaultRouter = lib.mkOption {
      type = lib.types.str;
      default = "osrm";
      description = "Default online router (upstream stadiamaps needs an API key).";
    };
    defaultGeocoder = lib.mkOption {
      type = lib.types.str;
      default = "photon";
    };
    defaultBasemap = lib.mkOption {
      type = lib.types.str;
      default = "OpenFreeMap Bright";
    };
    extraEnv = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Extra variables set for Pure Maps only.";
    };
    unsetEnv = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Variables removed from Pure Maps' environment.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ pureMaps ];
    system.build.fp6-pure-maps = pureMaps;
  };
}
