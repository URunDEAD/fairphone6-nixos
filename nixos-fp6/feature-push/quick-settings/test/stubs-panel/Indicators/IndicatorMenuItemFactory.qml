// Preview stub: the media player rows of the sound indicator page.
import QtQuick 2.15
import Lomiri.Components 1.3
QtObject {
    property string indicator
    property var rootModel
    property var menuModel
    property Component player: Component {
        Item {
            property var menuData
            property int menuIndex
            height: units.gu(9)
            Rectangle { x: units.gu(2); y: units.gu(1); width: units.gu(7); height: width; radius: units.gu(1); color: "#E95420"
                Icon { anchors.centerIn: parent; name: "media-record"; width: units.gu(3); height: width; color: "white" } }
            Column { x: units.gu(10.5); y: units.gu(2)
                Label { text: menuData ? menuData.label : ""; color: "#F7F7F7" }
                Label { text: "Artist — Album"; textSize: Label.Small; color: "#AEA79F" } }
        }
    }
    property Component playback: Component {
        Item {
            property var menuData
            property int menuIndex
            height: units.gu(6)
            Row { anchors.centerIn: parent; spacing: units.gu(5)
                Repeater { model: ["media-skip-backward", "media-playback-pause", "media-skip-forward"]
                    Icon { name: modelData; width: units.gu(3.5); height: width; color: "#F7F7F7" } } }
        }
    }
    function load(m) { return /media-player/.test(m.type) ? player : playback; }
}
