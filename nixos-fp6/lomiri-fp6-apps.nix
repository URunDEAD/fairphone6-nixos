{ pkgs, ... }:
let
  schemas = pkgs.lomiri.lomiri-schemas;
in {
  # Apps launched by the session need the shared Lomiri settings schemas.
  # Follow the desktop module's schema exposure through XDG_DATA_DIRS.
  environment.extraInit = ''
    export XDG_DATA_DIRS="''${XDG_DATA_DIRS:-}''${XDG_DATA_DIRS:+:}${schemas}/share/gsettings-schemas/${schemas.name}"
  '';
}
