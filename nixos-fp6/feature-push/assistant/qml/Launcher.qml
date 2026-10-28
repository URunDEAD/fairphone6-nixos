// Window root for the QML engine (MainView is an Item).
import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3

Window {
    id: window
    title: i18n.tr("Assistant")
    visible: true
    width: units.gu(50)
    height: units.gu(100)
    color: theme.palette.normal.background
    Main { anchors.fill: parent }
}
