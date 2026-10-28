// Preview stub of QMenuModel's QDBusActionGroup: fp6-charging-feedback's
// actions and indicator-sound's silent-mode, simulated by FakeBus (no D-Bus).
import QtQuick 2.15
QtObject {
    property int busType
    property string busName
    property string objectPath
    function start() {}
    function action(name) { return FakeBus.action(busName, name); }
}
