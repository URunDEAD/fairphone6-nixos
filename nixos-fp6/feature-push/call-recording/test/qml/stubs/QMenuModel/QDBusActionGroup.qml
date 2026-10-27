// Preview stub of QMenuModel's QDBusActionGroup: actions come from
// FakeActions.states ("name" -> state); missing names are invalid, as when
// the fp6-callrec service is not running.
import QtQuick 2.15
QtObject {
    property int busType
    property string busName
    property string objectPath
    function start() {}
    function action(name) { return FakeActions.get(name); }
}
