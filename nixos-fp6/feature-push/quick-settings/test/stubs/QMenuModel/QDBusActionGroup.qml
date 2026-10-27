// Preview stub of QMenuModel's QDBusActionGroup (fake states from FakeActions).
import QtQuick 2.15
QtObject {
    property int busType
    property string busName
    property string objectPath
    function start() {}
    function action(name) { return FakeActions.get(busName, name); }
}
