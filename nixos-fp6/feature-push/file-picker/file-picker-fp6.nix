# Lomiri-native file picker for non-Lomiri apps through xdg-desktop-portal.
# Notes, screenshots and the phone checklist:
# results/feature-push-20261024/file-picker/README.md
#
#  * xdg-desktop-portal (1.20) + xdg-desktop-portal-lomiri (./portal, Rust,
#    zbus): org.freedesktop.impl.portal.FileChooser. Each OpenFile /
#    SaveFile / SaveFiles starts fp6-file-picker, a Lomiri UI Toolkit 1.3 page
#    (Suru): Recent, Home, Documents, Downloads, Music, Pictures, Videos, SD
#    card / USB mounts, "Other apps" (content-hub ContentPeerPicker: File
#    Manager, Gallery, ...), the app's filters, multiple selection, folder
#    selection, save with a name field, overwrite confirmation, new folder,
#    choices. Results are file:// URIs.
#  * portals.conf: FileChooser = lomiri, everything else = none. Only the
#    FileChooser interface (plus xdp's backend-less monitors) is exported, so
#    GTK4 keeps reading its settings from /etc/xdg/gtk-4.0/settings.ini
#    (no Settings portal: gtk-lomiri-fp6.nix's decoration layout stays) and
#    keeps opening URIs itself (no OpenURI portal).
#  * Who uses it: GTK4 apps automatically (Fragments' "Open torrent", GtkFileDialog
#    uses the portal whenever org.freedesktop.portal.Desktop is activatable);
#    Firefox through widget.use-xdg-desktop-portal.file-picker=1 (set here);
#    Qt5/Qt6 non-UITK apps with QT_QPA_PLATFORMTHEME=xdgdesktopportal in their
#    wrapper. Morph (Qt6 UITK) already opens content-hub's ContentPeerPicker
#    itself and is unchanged.
#  * Window: the picker is its own Lomiri app window; DESKTOP_FILE_HINT points
#    qtmir at the NoDisplay fp6-file-picker.desktop (name "Files").
#  * AppArmor: the confined apps read what is picked through their own
#    profiles (fp6-user-media-read: Documents, Downloads, Music, Pictures,
#    Videos, /media). Content-hub copies land in
#    ~/.cache/fp6-file-picker/HubIncoming; read access to that is added to
#    fp6-user-media-read through its .d include directory. Files picked
#    elsewhere in $HOME are denied for enforce-mode profiles (Fragments).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.filePickerPortal;
  package = pkgs.callPackage ./package.nix { };
in {
  options.hardware.fairphone6.filePickerPortal = {
    enable = lib.mkEnableOption "the Lomiri file picker for GTK/Qt/Firefox apps (xdg-desktop-portal FileChooser backend)";
    package = lib.mkOption {
      type = lib.types.package;
      default = package;
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "xdg-desktop-portal-lomiri + fp6-file-picker.";
    };
    contentHub = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Show the "Other apps" section (content-hub source peers such as the
        File Manager and Gallery) in Open dialogs.
      '';
    };
    gtkFallback = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Also install xdg-desktop-portal-gtk and use it for every interface
        except FileChooser. Off: it would export a Settings portal (GTK4 then
        takes fonts, colour scheme and the decoration layout from GSettings
        instead of /etc/xdg/gtk-4.0/settings.ini), and its dialogs (Open
        With, Print) are desktop GTK windows started without WAYLAND_DISPLAY
        from the user manager.
      '';
    };
    firefox = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Set widget.use-xdg-desktop-portal.file-picker=1 for the Lomiri Firefox.";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      xdg.portal = {
        enable = true;
        extraPortals = [ cfg.package ] ++ lib.optional cfg.gtkFallback pkgs.xdg-desktop-portal-gtk;
        config = let
          prefs = {
            default = if cfg.gtkFallback then [ "gtk" ] else [ "none" ];
            "org.freedesktop.impl.portal.FileChooser" = [ "lomiri" ];
          };
        in {
          common = prefs;
          # XDG_CURRENT_DESKTOP=Lomiri -> lomiri-portals.conf is read first.
          lomiri = prefs;
        };
      };

      environment.systemPackages = [ cfg.package ];

      # xdg-document-portal (/run/user/$UID/doc, the paths handed to apps)
      # mounts its FUSE file system through fusermount3. This nixpkgs'
      # xdg.portal module no longer enables programs.fuse, so without the
      # setuid wrapper /run/wrappers/bin/fusermount3 the unit failed with
      # "Failed to call 'fusermount3'". The user manager's PATH already
      # contains /run/wrappers/bin.
      programs.fuse.enable = true;

      # The user manager gets NIX_XDG_DESKTOP_PORTAL_DIR from the PAM session
      # variables; restate it for the D-Bus activated units.
      systemd.user.services.xdg-desktop-portal.environment.NIX_XDG_DESKTOP_PORTAL_DIR =
        "/run/current-system/sw/share/xdg-desktop-portal/portals";
      systemd.user.services.xdg-desktop-portal-lomiri.environment = {
        FP6_PICKER_CONTENT_HUB = if cfg.contentHub then "1" else "0";
        # Lomiri's socket; the user manager has no WAYLAND_DISPLAY here.
        WAYLAND_DISPLAY = "wayland-0";
      };

      system.build.fp6-file-picker = cfg.package;
    }

    (lib.mkIf cfg.firefox {
      hardware.fairphone6.lomiriFirefox.extraPrefs = ''
        // fp6: file dialogs through xdg-desktop-portal (Lomiri file picker).
        pref("widget.use-xdg-desktop-portal.file-picker", 1);
      '';
    })

    (lib.mkIf (config.hardware.fairphone6.apparmor.enable or false) {
      security.apparmor.includes."abstractions/fp6-user-media-read.d/fp6-file-picker" = ''
        # Items handed over by content-hub ("Other apps" in the file picker).
        owner @{HOME}/.cache/fp6-file-picker/HubIncoming/{,**} r,
      '';
    })
  ]);
}
