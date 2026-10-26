# Native launchers from the recorded pmOS inventory. No account/data migration.
{ pkgs, browserPackage ? pkgs.firefox-esr }:
let
  names = [
    "gapless" "firefox-esr" "phosh-mobile-settings" "ibus" "malcontent-ui"
    "gnome-calculator" "gnome-calendar" "calls" "gnome-console"
    "gnome-contacts" "decibels" "loupe" "gnome-maps" "nautilus" "papers"
    "gnome-control-center" "showtime" "snapshot" "gnome-software"
    "gnome-text-editor" "gnome-weather" "gnome-clocks" "rygel"
  ];
  appOnly = name:
    let original = if name == "firefox-esr" then browserPackage else builtins.getAttr name pkgs;
    in if builtins.elem name [ "calls" "ibus" ] then pkgs.symlinkJoin {
      name = "${name}-fp6-app-only";
      paths = [ original ];
      # Calls ships a session autostart; IBus ships an enabled GNOME-session
      # dependency. Keep explicit launch/DBus units but never install auto-wants.
      postBuild = ''
        rm -rf "$out/etc/xdg/autostart" \
          "$out/share/systemd/user/gnome-session.target.wants"
      '';
      passthru = { inherit original; };
    } else original;
in {
  inherit names;
  packages = map appOnly names;
  pending = [ "lpa-gtk" "vvmplayer" "phosh-tour" "chatty" ];
}
