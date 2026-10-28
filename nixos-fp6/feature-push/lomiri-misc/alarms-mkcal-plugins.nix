# Only the QtOrganizer mkcal plugin, in its own plugin root, for
# QT_PLUGIN_PATH (lomiri-misc-fp6.nix `alarmsMkcal`). A dedicated root keeps
# the rest of the system profile's Qt plugins out of every app's search path.
{ pkgs }:
let
  prefix = pkgs.libsForQt5.qtbase.qtPluginPrefix;
in pkgs.runCommand "fp6-qt5-organizer-mkcal" { passthru.pluginRoot = "lib/fp6-qt5-organizer"; } ''
  mkdir -p $out/lib/fp6-qt5-organizer/organizer
  for f in ${pkgs.qtorganizer-mkcal}/${prefix}/organizer/*.so; do
    ln -s "$f" $out/lib/fp6-qt5-organizer/organizer/
  done
  test -e $out/lib/fp6-qt5-organizer/organizer/libqtorganizer_mkcal.so
''
