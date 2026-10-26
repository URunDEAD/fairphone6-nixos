// Small labelled value (caption above, value below).
import QtQuick 2.12
import Lomiri.Components 1.3

Column {
    property alias caption: cap.text
    property alias value: val.text
    property alias valueColor: val.color
    property int size: Label.Large
    spacing: units.gu(0.25)
    Label {
        id: cap
        textSize: Label.Small
        color: theme.palette.normal.backgroundSecondaryText
    }
    Label {
        id: val
        textSize: parent.size
        font.weight: Font.Light
    }
}
