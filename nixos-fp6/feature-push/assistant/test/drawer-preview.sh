#!/usr/bin/env bash
# Offscreen preview of the drawer's "Ask the assistant" row: the exact
# snippet the module inserts into Lomiri's Drawer.qml, inside a stand-in
# drawer (dark background, search field, app grid placeholder).
#   test/drawer-preview.sh ENV_DIR OUT.png
set -eu
env_dir=$1; out=$2
here=$(cd "$(dirname "$0")/.." && pwd)
w=$(mktemp -d); trap 'rm -rf "$w"' EXIT
{
cat <<'QML'
import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3
Window {
    width: units.gu(50); height: units.gu(40); visible: true; color: "#262626"
    Rectangle { anchors.fill: parent; color: "#262626" }
    FocusScope {
        id: root
        anchors.fill: parent
        property bool lightMode: false
        signal hideRequested()
        Item {
            id: contentContainer
            anchors.fill: parent
            Item {
                id: searchFieldContainer
                height: units.gu(4)
                anchors { left: parent.left; top: parent.top; right: parent.right; margins: units.gu(1) }
                TextField { id: searchField; anchors.fill: parent; text: "remind me to call Test at 5"; placeholderText: "Search…" }
            }
QML
cat "$here/lomiri/DrawerAskRow.qml.in"
cat <<'QML'
            Grid {
                anchors { left: parent.left; right: parent.right; top: fp6AskAssistant.bottom; margins: units.gu(2) }
                columns: 4; spacing: units.gu(2)
                Repeater { model: 4; Rectangle { width: units.gu(8); height: units.gu(8); radius: units.gu(2); color: "#555" } }
            }
        }
    }
    Timer { interval: 1500; running: true; onTriggered: contentItem.grabToImage(function(r) { r.saveToFile("@OUT@"); console.log(fp6AskAssistant.visible ? "PASS drawer row visible" : "FAIL drawer row hidden"); Qt.quit() }) }
}
QML
} | sed "s|@OUT@|$out|" > "$w/Drawer.qml"
XDG_RUNTIME_DIR="$w" HOME="$w" QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=16 timeout 30 "$env_dir/bin/fp6-assistant-env" \
  "${FP6_QMLSCENE:-qmlscene}" "$w/Drawer.qml" 2>&1 | grep -v "adwaita\|Available styles" || true
test -s "$out"
