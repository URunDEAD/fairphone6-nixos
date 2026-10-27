#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p previews "$work/runtime" "$work/data" "$work/config" "$work/cache"
chmod 700 "$work/runtime"
export QT_QPA_PLATFORM=offscreen QT_SCALE_FACTOR=1 GRID_UNIT_PX=8
unset QT_QUICK_BACKEND QT_QPA_OFFSCREEN_NO_GLX
export XDG_RUNTIME_DIR="$work/runtime" XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export GSETTINGS_BACKEND=memory QT_STYLE_OVERRIDE=Fusion LIBGL_ALWAYS_SOFTWARE=1 APP_ID=fp6-webshare
cargo run --locked --quiet --example preview_state > "$work/state.json"
python3 - "$PWD" "$work" <<'PY'
import json, pathlib, sys
root, temp = map(pathlib.Path, sys.argv[1:])
state = json.loads((temp / 'state.json').read_text())
for mode in ('idle', 'sharing', 'approval'):
    values = 'selectedFiles: ["/synthetic/Weekend notes.pdf", "/synthetic/Garden photo.jpg"]' if mode == 'idle' else 'stateData: ' + json.dumps(state)
    if mode == 'approval':
        state['claimed'] = True
        state['pending'] = [{'id':'synthetic','name':'Meeting notes.txt','size':2048}]
        state['text'] = 'See you at the park at 16:00.'
        values = 'stateData: ' + json.dumps(state)
    height = 1250 if mode == 'approval' else 960
    (temp / (mode + '.qml')).write_text('''import QtQuick 2.12
import %s as App
App.Main {
    id: main
    width: 432; height: %d
    previewMode: true
    networks: [{name: "wlan0", address: "192.168.77.1"}]
    %s
    Timer { interval: 1600; running: true; onTriggered: main.grabToImage(function(result) {
        if (!result.saveToFile(%s)) { console.error("Could not save preview"); Qt.quit(); }
        else Qt.quit();
    }) }
}
''' % (json.dumps((root / 'qml').as_uri()), height, values, json.dumps(str(root / 'previews' / (mode + '.png')))))
PY
for mode in idle sharing approval; do
  xvfb-run -a dbus-run-session -- timeout 45 qmlscene "$work/$mode.qml"
  test -s "previews/$mode.png"
done
