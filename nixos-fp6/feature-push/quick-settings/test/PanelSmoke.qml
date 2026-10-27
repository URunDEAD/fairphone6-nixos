// Laptop smoke test of the patched panel (real Panel.qml/PanelMenu.qml with
// Lomiri's own test mocks for the indicators): collapsed bar, pulled down
// (control centre), an indicator page, the lock-screen card. Screenshots and
// QML errors are the output; see panel-smoke.sh.
import QtQuick 2.15
import Lomiri.Components 1.3
import Lomiri.Indicators 0.1 as Indicators
import "Panel"
import "Components/PanelState"
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484
    Component.onCompleted: theme.name = "Lomiri.Components.Themes.SuruDark"

    Rectangle { anchors.fill: parent; gradient: Gradient { GradientStop { position: 0; color: "#2C3E50" } GradientStop { position: 1; color: "#7B4397" } } }
    PanelState { id: panelState }
    ListModel {
        id: shade
        ListElement { app: "Messages"; appId: "messaging-app"; icon: "image://theme/message"; summary: "Alex"; body: "See you at 7?"; time: 1793102400000 }
        ListElement { app: "Firefox"; appId: "firefox"; icon: "image://theme/browser-app"; summary: "Download complete"; body: "report.pdf"; time: 1793101800000 }
    }

    Panel {
        id: panel
        anchors.fill: parent
        panelState: panelState
        minimizedPanelHeight: units.gu(3)
        expandedPanelHeight: units.gu(7)
        fp6Notifications: shade
        indicators {
            model: Indicators.IndicatorsModel {
                id: indicatorsModel
                Component.onCompleted: load()
            }
        }
    }

    property int step: 0
    function save(name, next) {
        root.grabToImage(function(r) { r.saveToFile(Shot.out + "/" + name + ".png"); console.log("shot " + name); next(); });
    }
    function log(s) { console.log("CHECK " + s); }
    Timer {
        id: t
        interval: 1500
        running: true
        onTriggered: {
            var ind = panel.indicators;
            switch (root.step++) {
            case 0:
                log("safe inset collapsed=" + panel.fp6SafeInset + " expanded=" + panel.fp6ExpandedSafeInset
                    + " overFlowWidth=" + ind.overFlowWidth + " drag=" + ind.dragOverFlowWidth);
                save("01-collapsed", function() { ind.show(); t.restart(); });
                break;
            case 1:
                log("overlay shown=" + ind.fp6OverlayShown + " item=" + !!ind.fp6OverlayItem + " visible=" + (ind.fp6OverlayItem && ind.fp6OverlayItem.visible));
                save("02-control-centre", function() { ind.fp6OpenIndicator("fake-indicator-sound"); t.restart(); });
                break;
            case 2:
                log("after open sound page: overlay shown=" + ind.fp6OverlayShown + " current=" + ind.fp6CurrentIdentifier);
                save("03-indicator-page", function() { panel.fp6Locked = true; ind.fp6OpenIndicator("fake-indicator-network"); t.restart(); });
                break;
            case 3:
                log("locked current=" + ind.fp6CurrentIdentifier);
                save("04-locked-page", function() { ind.fp6OverlayShown = true; t.restart(); });
                break;
            case 4:
                save("05-locked-cc", function() { panel.fp6Locked = false; ind.hide(); t.restart(); });
                break;
            case 5:
                log("closed: state=" + ind.state + " overlay shown (reset)=" + ind.fp6OverlayShown
                    + " overlay visible=" + (ind.fp6OverlayItem && ind.fp6OverlayItem.visible));
                Qt.quit();
            }
        }
    }
}
