# Mobile Firefox in the Lomiri GPU session.
#
# ../../firefox-mobile.nix (hardware.fairphone6.mobileFirefox, already enabled
# lower in the chain) builds release Firefox with mobile-config-firefox, but it
# only reached the system through the postmarketOS app set, which the Lomiri
# target turns off. This module installs that same Firefox derivation
# (postmarketosApps.browserPackage), wrapped for Lomiri/Mir:
#
#  * Native Wayland: MOZ_ENABLE_WAYLAND=1 and GDK_BACKEND=wayland for Firefox
#    only. The session also exports DISPLAY=:0 (Xwayland), so Firefox would
#    otherwise be allowed to choose X11.
#  * Scale (verified on hardware 2026-10-24, see
#    results/feature-push-20261024/firefox-scaling/README.md):
#    GTK reads Mir's wl_output scale 3 itself (toplevel set_buffer_scale(3),
#    372x804 logical). GDK_SCALE is ignored by GTK3 on Wayland anyway; it and
#    GDK_DPI_SCALE are unset so nothing stacks a second factor on top.
#    Firefox draws its content into a wl_subsurface and, since the
#    WaylandSurface rewrite, sizes that subsurface ONLY through wp_viewporter
#    (EnableCeiledScaleLocked/EnableFractionalScaleLocked). Mir 2.15 does not
#    offer wp_viewporter (nor wp_fractional_scale_v1), so the 1116x2412 content
#    buffer was committed with buffer scale 1 = 1116x2412 logical: 3x too big,
#    the screen showed only the top-left ninth of the page (toolbar off-screen).
#    Fix 1 (default): firefox-subsurface-scale-shim.c, LD_PRELOADed by the
#    wrapper, gives that subsurface its parent's set_buffer_scale(3) before
#    each commit (Firefox processes only; no-op when a wp_viewport exists).
#    Verified on the phone: page fills the window, innerWidth = logical width,
#    devicePixelRatio 3, sharp text.
#    Fix 1b (bufferScaleFallback, off by default): the same fallback inside
#    Firefox (firefox-wayland-buffer-scale-fallback.patch). It needs a local
#    Firefox build; the 7 GiB / 10 GiB-free builder VM could not finish it
#    (PGO rustc SIGKILL, then ENOSPC while unpacking). Enable it on a bigger
#    builder and drop the shim.
#    Fix 2: mobile-config-firefox turns on browser.tabs.inTitlebar, so GTK
#    draws client-side decorations: 26/23 px transparent shadow margin
#    (surface 424x856 around a 372x804 window geometry) and rounded corners
#    showing the wallpaper. inTitlebar=0 makes GTK request server-side
#    decorations (org_kde_kwin_server_decoration mode 2); Lomiri draws none,
#    so the surface is exactly the window (geometry 0,0,372,804).
#    Known leftover (Lomiri, not Firefox): the first configure is in physical
#    px (1116x2412), so Firefox allocates one 3348x7236 buffer before the
#    correct 372x804 configure arrives.
#  * App switcher (spread) preview, 2026-10-26: Firefox's card was far too
#    tall while every other app looked right. Not a client bug: fp6's
#    lomiri-wayland-surface-scale.patch draws surfaces with
#    MirSurfaceItem.Stretch, and the spread shrinks each window item to a
#    square preview. qtmir stretched the root surface's texture into the
#    item but drew every wl_subsurface at its raw extent in surface
#    coordinates (372x804), so Firefox - all content in one subsurface -
#    kept its full size inside a shrunken card. qtmirSubsurfaceStretch
#    (qtmir-subsurface-stretch.patch) maps subsurfaces through the same
#    scale as the root texture (identity when item size == surface size).
#  * GPU: WebRender picks hardware GL through EGL on Wayland with Mesa
#    freedreno by default. Nothing forces software (check about:support ->
#    Compositing: WebRender, not "WebRender (Software)").
#  * OSK: the session exports GTK_IM_MODULE=maliit, but there is no Maliit GTK
#    IM module (no gtk-3.0 immodules on the phone), so GTK would fall back to
#    the simple context and never ask for the keyboard. Firefox gets
#    GTK_IM_MODULE=wayland, i.e. GTK3's built-in zwp_text_input_v3 client
#    (firefox-mobile.nix already sets widget.wayland.text-input-v3.enabled).
#    Mir enables text-input v1/v2/v3 by default, and fp6's
#    mir-input-method-v1.patch routes the TextInputHub to Maliit's
#    zwp_input_method_v1 (MIR_SERVER_ADD_WAYLAND_EXTENSIONS adds
#    zwp_input_panel_v1:zwp_input_method_v1). So text-input-v3 -> Mir ->
#    input-method-v1 -> Maliit is the expected path. It has not been tested
#    with a GTK client on the phone.
#  * Launcher: a firefox.desktop with an absolute Exec to the wrapper is
#    installed, so lomiri-app-launch lists and starts it like other legacy
#    (non-click) apps. X-Lomiri-Touch is left unset, so Lomiri's lifecycle
#    manager does not SIGSTOP Firefox in the background.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriFirefox;
  # The wrapped mobile Firefox from firefox-mobile.nix, plus the Lomiri prefs.
  # Autoconfig pref() sets a user value on every start, so it wins over
  # mobile-config-firefox's defaultPref('browser.tabs.inTitlebar', 1).
  base = config.hardware.fairphone6.postmarketosApps.browserPackage.override (old: {
    extraPrefs = (old.extraPrefs or "") + lib.optionalString cfg.serverSideDecorations ''
      // fp6 Lomiri: no GTK client-side shadow; Lomiri stretches the full surface.
      pref("browser.tabs.inTitlebar", 0);
    '' + cfg.extraPrefs;
  });
  shim = pkgs.stdenv.mkDerivation {
    name = "fp6-firefox-subsurface-scale-shim";
    src = ./firefox-subsurface-scale-shim.c;
    dontUnpack = true;
    buildInputs = [ pkgs.wayland ];
    buildPhase = ''
      $CC -shared -fPIC -O2 -Wall -o libfp6-subsurface-scale.so $src -ldl -lwayland-client
    '';
    installPhase = ''
      install -Dm755 libfp6-subsurface-scale.so $out/lib/libfp6-subsurface-scale.so
    '';
  };
  firefox = pkgs.symlinkJoin {
    name = "firefox-mobile-lomiri-fp6";
    paths = [ base ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      rm "$out/bin/firefox"
      makeWrapper ${base}/bin/firefox "$out/bin/firefox" \
        --set MOZ_ENABLE_WAYLAND 1 \
        --set GDK_BACKEND wayland \
        --set GTK_IM_MODULE ${lib.escapeShellArg cfg.gtkImModule} \
        --unset GDK_SCALE \
        --unset GDK_DPI_SCALE \
        ${lib.optionalString cfg.subsurfaceScaleShim "--prefix LD_PRELOAD : ${shim}/lib/libfp6-subsurface-scale.so"} \
        ${lib.concatStringsSep " " (lib.mapAttrsToList (k: v: "--set ${k} ${lib.escapeShellArg v}") cfg.extraEnv)}
      src=$(readlink -f "$out/share/applications/firefox.desktop")
      rm "$out/share/applications/firefox.desktop"
      sed -e "s|^Exec=firefox|Exec=$out/bin/firefox|" \
          -e "s|^Exec=[^ ]*/bin/firefox|Exec=$out/bin/firefox|" \
          -e '/^\[Desktop Entry\]$/a X-Lomiri-Touch=true' \
          "$src" > "$out/share/applications/firefox.desktop"
      grep -q "^Exec=$out/bin/firefox" "$out/share/applications/firefox.desktop"
      # Lomiri's app drawer lists only touch-enabled launchers.
      test "$(grep -c '^X-Lomiri-Touch=true$' "$out/share/applications/firefox.desktop")" = 1
    '';
    meta = base.meta or { };
  };
in {
  options.hardware.fairphone6.lomiriFirefox = {
    enable = lib.mkEnableOption "the mobile Firefox (firefox-mobile.nix) wrapped for Lomiri/Mir";
    gtkImModule = lib.mkOption {
      type = lib.types.str;
      default = "wayland";
      description = "GTK_IM_MODULE for Firefox (wayland = text-input-v3).";
    };
    subsurfaceScaleShim = lib.mkOption {
      type = lib.types.bool;
      default = !cfg.bufferScaleFallback;
      description = ''
        LD_PRELOAD a small libwayland-client shim into Firefox that applies the
        parent's wl_surface.set_buffer_scale to Firefox's content subsurface
        (Mir 2.15 has no wp_viewporter). Off when bufferScaleFallback is on.
      '';
    };
    bufferScaleFallback = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Patch firefox-unwrapped to use wl_surface.set_buffer_scale when the
        compositor has no wp_viewporter (Mir 2.15). Without it Firefox's
        content is shown at 3x size on the FP6.
      '';
    };
    serverSideDecorations = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Set browser.tabs.inTitlebar=0 so GTK draws no CSD shadow margin.";
    };
    qtmirSubsurfaceStretch = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Patch qtmir so wl_subsurfaces are scaled together with the main
        surface texture when the MirSurfaceItem is not at surface size
        (Lomiri's spread preview). Without it Firefox's switcher card is
        shown with unscaled content, far taller than the card.
      '';
    };
    extraEnv = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = { MOZ_ENABLE_WAYLAND = "1"; };
      description = "Extra variables set for Firefox only.";
    };
    extraPrefs = lib.mkOption {
      type = lib.types.lines;
      default = "";
      example = ''pref("widget.use-xdg-desktop-portal.file-picker", 1);'';
      description = "Extra autoconfig lines (pref(...)) appended to Firefox's prefs.";
    };
  };

  config = lib.mkIf cfg.enable {
    hardware.fairphone6.mobileFirefox.enable = true;
    nixpkgs.overlays = lib.mkMerge [ (lib.mkIf cfg.bufferScaleFallback [ (final: prev: {
      # The patch forces a local build (no cache hit). The aarch64 builder VM
      # has 7 GiB RAM and no swap: the PGO instrumented build was SIGKILLed
      # (rustc firefox-on-glean, 2026-10-24), and LTO linking libxul needs far
      # more. Build without PGO/LTO; runtime cost is small next to an unusable
      # browser. Set both back to true on a bigger builder.
      firefox-unwrapped = (prev.firefox-unwrapped.override {
        pgoSupport = false;
        ltoSupport = false;
      }).overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./firefox-wayland-buffer-scale-fallback.patch ];
      });
    }) ])
    (lib.mkIf cfg.qtmirSubsurfaceStretch (lib.mkAfter [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (lfinal: lprev: {
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./qtmir-subsurface-stretch.patch ];
        });
      });
    }) ])) ];
    environment.systemPackages = [ firefox ];
    system.build.fp6-lomiri-firefox = firefox;
    system.build.fp6-lomiri-firefox-scale-shim = shim;
    system.build.fp6-lomiri-qtmir = pkgs.lomiri.qtmir;
    assertions = [{
      assertion = config.hardware.fairphone6.mobileFirefox.enable
        && !(config.environment.sessionVariables ? MOZ_ENABLE_WAYLAND);
      message = "fp6 lomiriFirefox: needs mobileFirefox and keeps MOZ_ENABLE_WAYLAND per-app.";
    }];
  };
}
