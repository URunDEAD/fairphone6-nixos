# Laptop test for lomiri-misc-fp6.nix `alarmsMkcal` (no phone):
#   nix-build nixos-fp6/feature-push/lomiri-misc/alarms-mkcal-test.nix
# One process sets an alarm (as the Clock app does), a second lists alarms:
#  * stock (no ALARM_BACKEND, no mkcal plugin on QT_PLUGIN_PATH): UITK warns
#    'alarm manager "eds" not installed, using "memory"' and keeps the alarm
#    in the app's own ~/.local/share/<app>/alarms.json. Nothing reaches the
#    mkcal database, so indicator-datetime (which rings alarms, mkcal engine)
#    and other apps never see it; without that JSON file it is gone.
#  * fixed (ALARM_BACKEND=mkcal + the plugin root): the alarm is in the mkcal
#    database, notebook "Alarms" (what indicator-datetime's mkcal engine
#    treats as alarms), and the second process lists it from there.
{ pkgs ? import (import ../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  plugins = import ./alarms-mkcal-plugins.nix { inherit pkgs; };
  qmlPath = pkgs.lib.makeSearchPath q.qtbase.qtQmlPrefix [
    q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin q.qtpim q.qtfeedback pkgs.lomiri.lomiri-ui-toolkit ];
  basePlugins = pkgs.lib.makeSearchPath q.qtbase.qtPluginPrefix [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtpim ];
  save = pkgs.writeText "save.qml" ''
    import QtQuick 2.12
    import Lomiri.Components 1.3
    Item {
        Alarm { id: alarm; message: "fp6 alarm test"; type: Alarm.OneTime; enabled: true }
        Component.onCompleted: { alarm.date = new Date(Date.now() + 3600 * 1000); alarm.save() }
        Timer { interval: 3000; running: true; onTriggered: { console.log("SAVED error=" + alarm.error); Qt.quit() } }
    }
  '';
  list = pkgs.writeText "list.qml" ''
    import QtQuick 2.12
    import Lomiri.Components 1.3
    Item {
        AlarmModel { id: model }
        Timer {
            interval: 3000; running: true
            onTriggered: {
                var names = []
                for (var i = 0; i < model.count; i++) names.push(model.get(i).message)
                console.log("ALARMS " + model.count + " [" + names.join(",") + "]")
                Qt.quit()
            }
        }
    }
  '';
in pkgs.runCommand "fp6-alarms-mkcal-test" { nativeBuildInputs = [ q.qtdeclarative.dev pkgs.sqlite ]; } ''
  export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software QML2_IMPORT_PATH=${qmlPath}
  export XDG_RUNTIME_DIR=$TMPDIR/run; mkdir -m700 -p $XDG_RUNTIME_DIR
  run() { timeout 60 qml "$1" > "$2" 2>&1 || true; cat "$2"; }

  echo "== stock"
  export HOME=$TMPDIR/stock; mkdir -p $HOME
  unset ALARM_BACKEND
  export QT_PLUGIN_PATH=${basePlugins}
  run ${save} stock-save.log
  grep -q 'alarm manager "eds" not installed, using "memory"' stock-save.log
  find $HOME -name alarms.json | tee stock-json.txt | grep -q .
  ! find $HOME -path '*mkcal*' -type f | grep -q .
  # The app-private JSON is the only copy.
  find $HOME -name alarms.json -delete
  run ${list} stock-list.log
  grep -q 'ALARMS 0 \[\]' stock-list.log

  echo "== fixed"
  export HOME=$TMPDIR/fixed; mkdir -p $HOME
  export ALARM_BACKEND=mkcal
  export QT_PLUGIN_PATH=${basePlugins}:${plugins}/${plugins.pluginRoot}
  run ${save} fixed-save.log
  run ${list} fixed-list.log
  ! grep -q 'not installed' fixed-save.log
  grep -q 'SAVED error=0' fixed-save.log
  ! find $HOME -name alarms.json | grep -q .
  grep -q 'ALARMS 1 \[fp6 alarm test\]' fixed-list.log
  db=$(find $HOME -path '*mkcal*' -name db -type f | head -1)
  test -n "$db"
  sqlite3 "$db" "SELECT c.Name, i.Summary FROM Components i JOIN Calendars c ON c.CalendarId = i.Notebook;" | tee fixed-db.txt
  grep -qx 'Alarms|fp6 alarm test' fixed-db.txt

  mkdir -p $out
  cp *.log $out/
  cp fixed-db.txt $out/
''
