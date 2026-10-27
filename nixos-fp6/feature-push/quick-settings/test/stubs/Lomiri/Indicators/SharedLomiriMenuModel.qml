// Preview stub of Lomiri.Indicators.SharedLomiriMenuModel.
import QtQuick 2.15
QtObject {
    id: m
    property string busName
    property var actions
    property string menuObjectPath
    property ListModel sub: ListModel {}
    property QtObject model: QtObject {
        function submenu(i) { return m.sub; }
    }
    Component.onCompleted: {
        var r = FakeMenus.rows[busName] || [];
        for (var i = 0; i < r.length; i++) sub.append(r[i]);
    }
}
