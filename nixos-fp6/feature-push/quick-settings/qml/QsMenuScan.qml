/*
 * FP6 quick settings: reads one label out of an indicator's menu (the shared,
 * cached menu model the indicator page uses, so no extra D-Bus traffic):
 * the first row of the indicator's main submenu whose type/action matches
 * and which is toggled on (the connected Wi-Fi network, a connected
 * Bluetooth device). Only active while the control centre is visible.
 */
import QtQuick 2.15
import Lomiri.Indicators 0.1 as Indicators

Item {
    id: scan
    property var info: null          // {busName, actionsObjectPath, menuObjectPath}
    property bool scanning: false
    property string typePattern: ""
    property string actionPattern: ""
    readonly property string label: d.label

    QtObject {
        id: d
        property string label: ""
        property var rows: []
        function recompute() {
            var tre = scan.typePattern ? new RegExp(scan.typePattern) : null;
            var are = scan.actionPattern ? new RegExp(scan.actionPattern) : null;
            for (var i = 0; i < rows.length; i++) {
                var r = rows[i];
                if (!r || !r.on) continue;
                if (tre && !tre.test(r.type)) continue;
                if (are && !are.test(r.action)) continue;
                label = r.label.replace(/_/g, "");
                return;
            }
            label = "";
        }
    }

    Loader {
        active: scan.scanning && !!scan.info && !!scan.info.busName
        sourceComponent: Item {
            Indicators.SharedLomiriMenuModel {
                id: cached
                busName: scan.info.busName
                actions: { "indicator": scan.info.actionsObjectPath }
                menuObjectPath: scan.info.menuObjectPath
            }
            Instantiator {
                model: cached.model ? cached.model.submenu(0) : null
                delegate: QtObject {
                    readonly property string type: model.type || ""
                    readonly property string action: model.action || ""
                    readonly property bool on: !!model.isToggled
                    readonly property string label: model.label || ""
                    onOnChanged: Qt.callLater(d.recompute)
                    onLabelChanged: Qt.callLater(d.recompute)
                }
                onObjectAdded: { var r = d.rows.slice(); r.splice(index, 0, object); d.rows = r; Qt.callLater(d.recompute); }
                onObjectRemoved: { var r = d.rows.slice(); r.splice(index, 1); d.rows = r; Qt.callLater(d.recompute); }
            }
            Component.onDestruction: { d.rows = []; d.label = ""; }
        }
    }
}
