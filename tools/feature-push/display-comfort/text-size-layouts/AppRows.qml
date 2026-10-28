// Layout check of two fixed-height list rows at a given text size, with the
// geometry of the installed apps' QML (anchors, heights and label sizes
// copied; data synthetic):
//  * Phone: HistoryPage/HistoryDelegate.qml (8 gu row, 2 gu label boxes).
//  * Messages: ThreadDelegate.qml (8 gu row, 6 gu content, name + two-line
//    preview).
// patched: true = the Messages row of messaging-text-size.patch (the Phone
// row passes unpatched and stays stock). Prints LAYOUT-RESULT and saves grab.png.
import QtQuick 2.12
import Lomiri.Components 1.3

Rectangle {
    id: root
    width: 1116
    height: 900
    color: "white"
    property bool patched: false
    readonly property real textScale: Math.max(1, FontUtils.sizeToPixels("medium") / units.dp(14))

    Column {
        width: parent.width
        // ---- Phone: call history row ----
        Rectangle {
            id: history
            width: parent.width
            color: "#f4f4f4"
            height: units.gu(8)
            Rectangle {
                id: hAvatar; color: "#c0c0c0"
                anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
                width: height
            }
            Label {
                id: hTitle
                anchors { top: parent.top; topMargin: units.gu(0.5); left: hAvatar.right; leftMargin: units.gu(2); right: hTime.left; rightMargin: units.gu(1) }
                height: units.gu(2)
                verticalAlignment: Text.AlignTop
                fontSize: "medium"
                text: "Ana-Maria Popescu"
                elide: Text.ElideRight
            }
            Label {
                id: hPhone
                anchors { top: hTitle.bottom; topMargin: units.gu(1); left: hAvatar.right; leftMargin: units.gu(2) }
                height: units.gu(2)
                verticalAlignment: Text.AlignTop
                fontSize: "small"
                text: "Mobile"
            }
            Label {
                id: hTime
                anchors { right: parent.right; bottom: hTitle.bottom }
                height: units.gu(2)
                verticalAlignment: Text.AlignBottom
                fontSize: "small"
                text: "14:32"
            }
            Label {
                id: hType
                anchors { right: parent.right; bottom: hPhone.bottom }
                height: units.gu(2)
                verticalAlignment: Text.AlignBottom
                fontSize: "small"
                text: "Incoming"
            }
        }
        Rectangle { width: parent.width; height: 2; color: "black" }
        Item { width: 1; height: units.gu(2) }
        // ---- Messages: thread row (content inside 1 gu margins) ----
        Rectangle {
            id: thread
            width: parent.width
            color: "#f4f4f4"
            height: root.patched ? Math.max(units.gu(8), units.gu(2) + units.gu(0.5) + tName.height + units.gu(0.5) + tMsg.height)
                                 : units.gu(8)
            Item {
                id: tContent
                anchors { fill: parent; margins: units.gu(1) }
                Rectangle { id: tAvatar; color: "#c0c0c0"; anchors { left: parent.left; top: parent.top }
                            height: units.gu(6); width: units.gu(6) }
                Label {
                    id: tName
                    anchors { top: tAvatar.top; topMargin: units.gu(0.5); left: tAvatar.right; leftMargin: units.gu(3); right: tTime.left; rightMargin: units.gu(2) }
                    elide: Text.ElideRight
                    text: "Ana-Maria Popescu"
                }
                Label {
                    id: tTime
                    anchors { verticalCenter: tName.verticalCenter; right: parent.right }
                    text: "Yesterday"
                    fontSize: "small"
                }
                Label {
                    id: tMsg
                    anchors { top: tName.bottom; topMargin: units.gu(0.5); left: tName.left; right: tTime.left; rightMargin: units.gu(3) }
                    elide: Text.ElideRight
                    fontSize: "x-small"
                    text: "See you at the station at eight, the train leaves at quarter past and I have the tickets already."
                    maximumLineCount: 2
                    wrapMode: Text.Wrap
                }
            }
        }
        Rectangle { width: parent.width; height: 2; color: "black" }
    }

    function bottomOf(item, ref) { return item.mapToItem(ref, 0, 0).y + (item.verticalAlignment === Text.AlignBottom ? item.height : item.contentHeight) }
    function topOf(item, ref) { return item.mapToItem(ref, 0, 0).y + (item.verticalAlignment === Text.AlignBottom ? item.height - item.contentHeight : 0) }

    Timer {
        interval: 500; running: true
        onTriggered: {
            var r = {
                scale: root.textScale, patched: root.patched,
                // Phone: title text into the subtitle; any text below the row.
                historyTitleIntoSubtitle: Math.max(0, bottomOf(hTitle, history) - topOf(hPhone, history)),
                historyTimeIntoType: Math.max(0, bottomOf(hTime, history) - topOf(hType, history)),
                historyBelowRow: Math.max(0, Math.max(bottomOf(hPhone, history), bottomOf(hType, history)) - history.height),
                historyAboveRow: Math.max(0, -topOf(hTime, history)),
                // Messages: preview below the row (into the next thread).
                threadBelowRow: Math.max(0, bottomOf(tMsg, thread) - thread.height),
                rowHeights: [history.height, thread.height]
            };
            console.log("LAYOUT-RESULT " + JSON.stringify(r));
            root.grabToImage(function (g) { g.saveToFile("grab.png"); Qt.quit(); });
        }
    }
}
