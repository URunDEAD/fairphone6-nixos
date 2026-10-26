# Folders in Lomiri's app drawer (2026-10-26).
# Notes: results/feature-push-20261024/drawer-folders/README.md.
#
#  * Membership comes from the desktop entry:  X-Lomiri-Folder=<folder id>
#    (e.g. X-Lomiri-Folder=Tools). All apps with the same id are one folder
#    tile in the drawer grid (mini icons of its first four apps, sorted with
#    the apps by name). Tap opens the folder above the grid; tap outside,
#    Escape or closing the drawer closes it. Searching lists the apps inside
#    folders directly. Launching/pinning is unchanged (the folder uses the
#    drawer's own app delegate and applicationSelected()).
#  * Name and icon of a folder: desktop-directories/<folder id>.directory in
#    the XDG data dirs (Name=, Name[xx]=, Icon=); without it the id is the
#    name and the tile shows the mini icons. This module writes that file for
#    every hardware.fairphone6.drawerFolders.<id>.
#  * hardware.fairphone6.drawerFolders.<id>.apps = [ "desktop ids" ] puts
#    existing apps into a folder without repackaging them: the system profile's
#    share/applications/<id>.desktop symlink is replaced by a copy with the
#    X-Lomiri-Folder line (environment.extraSetup, i.e. inside system.path).
#
# Patch order: after every other Lomiri patch (lomiri-polish 2000,
# osk-wayland 2100, lomiri-multiwindow 2200, ...): lib.mkOrder 2500.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriDrawerFolders;
  folders = config.hardware.fairphone6.drawerFolders;

  folderType = lib.types.submodule ({ name, ... }: {
    options = {
      name = lib.mkOption {
        type = lib.types.str;
        default = name;
        description = "Folder name shown in the drawer (Name= of its .directory file).";
      };
      translations = lib.mkOption {
        type = lib.types.attrsOf lib.types.str;
        default = { };
        example = { ro = "Unelte"; de = "Werkzeuge"; };
        description = "Localised folder names (Name[<locale>]=).";
      };
      icon = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Icon name or absolute path for the folder tile; null = mini icons of its apps.";
      };
      apps = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        example = [ "lomiri-calculator-app" "lomiri-clock-app" ];
        description = ''
          Desktop file ids (without .desktop) of installed apps to put into this
          folder. Their entries in the system profile get X-Lomiri-Folder=<id>
          (an app's own X-Lomiri-Folder line is replaced). Each must exist in
          the system profile (checked at build).
        '';
      };
    };
  });

  directoryFile = id: f: ''
    [Desktop Entry]
    Type=Directory
    Name=${f.name}
  '' + lib.concatStrings (lib.mapAttrsToList (l: n: "Name[${l}]=${n}\n") f.translations)
     + lib.optionalString (f.icon != null) "Icon=${f.icon}\n";

  directories = pkgs.runCommand "fp6-lomiri-drawer-folders" { } (''
    mkdir -p $out/share/desktop-directories
  '' + lib.concatStrings (lib.mapAttrsToList (id: f: ''
    cp ${pkgs.writeText "${id}.directory" (directoryFile id f)} $out/share/desktop-directories/${id}.directory
  '') folders));

  # Runs inside system.path's buildEnv ($out = the system profile).
  assignApps = lib.concatStrings (lib.flatten (lib.mapAttrsToList (folder: f: map (app: ''
    fp6_folder_assign ${lib.escapeShellArg app} ${lib.escapeShellArg folder}
  '') f.apps) folders));
in {
  options.hardware.fairphone6 = {
    # On by default: importing the module is the switch (trial-lomiri-full-fp6.nix).
    lomiriDrawerFolders.enable = lib.mkEnableOption "folders in Lomiri's app drawer (desktop key X-Lomiri-Folder)"
      // { default = true; };
    drawerFolders = lib.mkOption {
      type = lib.types.attrsOf folderType;
      default = { };
      example = lib.literalExpression ''{ Tools.apps = [ "lomiri-calculator-app" ]; }'';
      description = ''
        App drawer folders, by folder id (the X-Lomiri-Folder value; letters,
        digits, '.', '_', '-'). Apps that carry X-Lomiri-Folder=<id> in their
        own desktop entry need no entry in `apps`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 2500 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-drawer-folders.patch ];
          passthru = (old.passthru or { }) // { fp6DrawerFolders = true; };
        });
      });
    }) ];

    environment.systemPackages = lib.mkIf (folders != { }) [ directories ];
    environment.pathsToLink = [ "/share/desktop-directories" ];

    environment.extraSetup = lib.mkIf (assignApps != "") ''
      fp6_folder_assign() {
        local f="$out/share/applications/$1.desktop"
        if [ ! -e "$f" ]; then
          echo "drawerFolders: $1.desktop (folder $2) is not in the system profile" >&2
          exit 1
        fi
        # buildEnv may have linked the whole directory from one package.
        if [ -L "$out/share/applications" ]; then
          local d; d=$(readlink -f "$out/share/applications")
          rm "$out/share/applications"; mkdir "$out/share/applications"
          ln -s "$d"/* "$out/share/applications/"
        fi
        local src; src=$(readlink -f "$f")
        rm "$f"
        # X-Lomiri-Folder goes into [Desktop Entry] (first group), replacing an existing one.
        ${pkgs.gawk}/bin/awk -v folder="$2" '
          /^\[/ { if (main && !done) { print "X-Lomiri-Folder=" folder; done = 1 }
                  main = ($0 == "[Desktop Entry]") }
          main && /^X-Lomiri-Folder[ \t]*=/ { next }
          { print }
          END { if (main && !done) print "X-Lomiri-Folder=" folder }
        ' "$src" > "$f"
      }
      ${assignApps}
    '';

    system.build.fp6-drawer-folders-lomiri = pkgs.lomiri.lomiri;

    assertions = [{
      assertion = lib.all (id: builtins.match "[A-Za-z0-9._-]+" id != null) (lib.attrNames folders);
      message = "hardware.fairphone6.drawerFolders: folder ids may only use letters, digits, '.', '_' and '-'.";
    }];
  };
}
