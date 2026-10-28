// Preview stub of Lomiri's Components/NotificationAudio.qml: logs plays.
import QtQuick 2.15
import QMenuModel 1.0
Item {
    property string source: ""
    function play() { FakeBus.note("sound " + String(source).split("/").pop()); }
    function stop() {}
}
