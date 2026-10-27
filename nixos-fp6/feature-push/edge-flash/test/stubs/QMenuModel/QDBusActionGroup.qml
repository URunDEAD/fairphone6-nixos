// Preview stub of QMenuModel's QDBusActionGroup: the EdgeFlash actions of
// fp6-lomiri-powerd, simulated by FakeEdge (no D-Bus).
import QtQuick 2.15
QtObject {
    property int busType
    property string busName
    property string objectPath
    function start() {}
    function action(name) { return FakeEdge.actions[name]; }
}
