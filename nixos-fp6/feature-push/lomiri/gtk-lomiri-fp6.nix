# GTK3 / GTK4 (libadwaita) apps as phone apps under Lomiri, plus the spread
# card aspect fix. Notes: results/feature-push-20261024/gtk-lomiri/README.md.
#
#  * Window buttons. GTK draws minimise/maximise/close from the
#    gtk-decoration-layout setting. There is no xdg-desktop-portal in this
#    session (nothing installed, nothing running), and GTK 4.22 on Wayland
#    reads GNOME settings ONLY through the Settings portal (the old direct
#    GSettings fallback is gone), so GTK4 used GtkSettings' built-in default
#    "menu:minimize,maximize,close" (what Fragments showed). GTK4 still reads
#    $XDG_CONFIG_DIRS/gtk-4.0/settings.ini, so /etc/xdg/gtk-4.0/settings.ini
#    sets the layout to ":" (no buttons on either side). GTK3 on Wayland falls
#    back to GSettings (org.gnome.desktop.wm.preferences button-layout, which
#    wins over settings.ini) when the schema is in XDG_DATA_DIRS, so the same
#    value goes into a system dconf database and into gtk-3.0/settings.ini for
#    apps without the schema. Apps are closed from the spread (swipe up), as
#    for every other Lomiri app; libadwaita dialogs keep their own close/back
#    buttons (those are not window controls).
#  * Full screen. Lomiri's staged (phone) stage sizes each window to the stage
#    but left the Mir state "restored", so Mir's xdg_toplevel.configure carried
#    no maximized/fullscreen state and GTK drew a floating CSD window: shadow
#    margin around the configured size, rounded corners, resize borders.
#    libadwaita always uses CSD (it sets a titlebar widget), so the Firefox
#    trick (ask for server-side decorations) does not work for it.
#    lomiri-staged-maximized.patch makes StagedFullscreenPolicy request
#    Mir.MaximizedState for staged windows (FullscreenState for low-chrome
#    apps, as before). Mir then sends xdg_toplevel state "maximized", GTK drops
#    shadow, corners and resize margins. Shell-requested state changes do not
#    move or resize the window in miral, and the staged delegate state ignores
#    windowState, so the shell geometry is unchanged. Leaving staged mode
#    restores the windows the policy maximized.
#  * Spread cards. fp6's lomiri-wayland-surface-scale.patch switched the app's
#    MirSurfaceItem to Stretch because qtmir's PadOrCrop compared the item
#    size (logical, 372x804) with the buffer size (physical, 1116x2412 at
#    buffer scale 3) and showed only the top-left ninth. Stretch however also
#    squeezed the whole window into the spread's square card.
#    qtmir-scaled-pad-or-crop.patch makes PadOrCrop work in surface (logical)
#    coordinates: each renderable (main surface and subsurfaces) is drawn 1:1
#    at its extent and clipped to the item. At full size that is exactly the
#    window (also clipping any CSD shadow outside the window geometry); in the
#    spread the card shows the top of the window with its own aspect ratio,
#    like upstream. The app SurfaceContainer goes back to PadOrCrop. Firefox's
#    content subsurface is clipped the same way, so it stays aligned.
#    qtmir-subsurface-stretch.patch (firefox-lomiri-fp6.nix) is kept; it only
#    acts in Stretch mode, which other MirSurfaceItems still use.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriGtk;
  settingsIni = ''
    [Settings]
    gtk-decoration-layout=${cfg.decorationLayout}
    gtk-titlebar-double-click=none
    gtk-titlebar-middle-click=none
  '';
in {
  options.hardware.fairphone6.lomiriGtk = {
    enable = lib.mkEnableOption "phone-style GTK3/GTK4 windows under Lomiri (no window buttons, maximized staged windows)";
    decorationLayout = lib.mkOption {
      type = lib.types.str;
      default = ":";
      description = ''
        gtk-decoration-layout / org.gnome.desktop.wm.preferences button-layout.
        ":" shows no window buttons. Use ":close" to keep a close button.
      '';
    };
    stagedMaximized = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Patch Lomiri so staged (phone) windows are in Mir's maximized state,
        so GTK drops its CSD shadow margin, rounded corners and resize borders.
      '';
    };
    spreadAspectCrop = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Patch qtmir's PadOrCrop fill mode to crop in logical surface
        coordinates and use it for app windows, so spread cards show the
        window's top part at its own aspect ratio instead of a squeezed window.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.etc."xdg/gtk-3.0/settings.ini".text = settingsIni;
    environment.etc."xdg/gtk-4.0/settings.ini".text = settingsIni;

    programs.dconf.enable = true;
    programs.dconf.profiles.user.databases = [{
      settings."org/gnome/desktop/wm/preferences" = {
        button-layout = cfg.decorationLayout;
        action-double-click-titlebar = "none";
        action-middle-click-titlebar = "none";
      };
    }];

    nixpkgs.overlays = lib.mkAfter [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ])
            ++ lib.optional cfg.stagedMaximized ./lomiri-staged-maximized.patch;
          # fillMode: Stretch comes from ../../lomiri-wayland-surface-scale.patch.
          postPatch = (old.postPatch or "") + lib.optionalString cfg.spreadAspectCrop ''
            substituteInPlace qml/Stage/SurfaceContainer.qml \
              --replace-fail "fillMode: MirSurfaceItem.Stretch" "fillMode: MirSurfaceItem.PadOrCrop"
          '';
        });
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or [ ])
            ++ lib.optional cfg.spreadAspectCrop ./qtmir-scaled-pad-or-crop.patch;
        });
      });
    }) ];

    system.build.fp6-lomiri-gtk-shell = pkgs.lomiri.lomiri;
    system.build.fp6-lomiri-gtk-qtmir = pkgs.lomiri.qtmir;
  };
}
