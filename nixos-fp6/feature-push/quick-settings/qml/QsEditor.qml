/*
 * FP6 quick settings: edit mode (Android "Edit tiles" / iOS Control Centre
 * customisation). Hold a tile and drag it to reorder, drag it down into the
 * tray to remove it, drag a tray tile up to add it; tap a tile to switch it
 * between small and wide; -/+ badges do the same without dragging. Tiles per
 * row, rows shown collapsed, the sliders and "Reset to default" are below.
 * Every change is written to com.fairphone6.quick-settings at once.
 * Not available on the lock screen (the control centre closes it).
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import "QsLayout.js" as QsLayout

Item {
    id: editor
    property QtObject backend
    property bool lightMode: false
    signal done()

    readonly property color fgColor: lightMode ? "#111111" : "#F7F7F7"
    readonly property color fg2Color: lightMode ? "#5D5D5D" : "#AEA79F"
    readonly property color cardColor: lightMode ? Qt.rgba(0, 0, 0, 0.06) : Qt.rgba(1, 1, 1, 0.09)

    property var working: []
    property var wide: []
    function load() {
        working = QsLayout.toList(backend.tiles);
        wide = QsLayout.toList(backend.wideTiles);
    }
    function commit() {
        if (!QsLayout.sameList(working, QsLayout.toList(backend.tiles)) ||
            !QsLayout.sameList(wide, QsLayout.toList(backend.wideTiles)))
            backend.setTiles(working, wide);
    }
    Component.onCompleted: load()

    readonly property var trayIds: {
        var out = [];
        for (var i = 0; i < backend.catalogue.length; i++) {
            var id = backend.catalogue[i].tileId;
            if (working.indexOf(id) < 0) out.push(id);
        }
        return out;
    }
    readonly property int columns: backend.columns
    readonly property real cellWidth: (width - units.gu(4)) / columns
    readonly property real diameter: Math.min(cellWidth - units.gu(2), units.gu(6.5))
    readonly property real cellHeight: diameter + units.gu(QsLayout.labelLines(columns) > 1 ? 5 : 3.5)
    readonly property var activeLay: QsLayout.layout(working, wide, columns, cellWidth, cellHeight)
    readonly property var trayLay: QsLayout.layout(trayIds, [], columns, cellWidth, cellHeight)

    // ---- dragging
    property string dragId: ""
    property real dragX: 0
    property real dragY: 0
    function beginDrag(id, p) { dragId = id; dragX = p.x; dragY = p.y; }
    function moveDrag(p) {
        dragX = p.x; dragY = p.y;
        if (p.y < col.y + trayHeader.y + units.gu(1)) {
            var rest = QsLayout.without(working, dragId);
            var lay = QsLayout.layout(rest, wide, columns, cellWidth, cellHeight);
            var idx = QsLayout.dropIndex(rest, lay.pos, p.x - col.x - activeArea.x, p.y - col.y - activeArea.y);
            var next = QsLayout.inserted(working, dragId, idx);
            if (!QsLayout.sameList(next, working)) working = next;
        } else if (working.indexOf(dragId) >= 0) {
            working = QsLayout.without(working, dragId);
        }
    }
    function endDrag() { dragId = ""; commit(); }
    function toggleWide(id) {
        var t = backend.tile(id);
        if (!t || !t.canWide) return;
        wide = wide.indexOf(id) >= 0 ? QsLayout.without(wide, id) : wide.concat([id]);
        commit();
    }
    function remove(id) { working = QsLayout.without(working, id); commit(); }
    function add(id) { working = working.concat([id]); commit(); }

    // ---- sliders
    function sliderList() {
        var shown = QsLayout.toList(backend.sliders), out = shown.slice();
        for (var i = 0; i < backend.allSliders.length; i++)
            if (out.indexOf(backend.allSliders[i]) < 0) out.push(backend.allSliders[i]);
        return out;
    }
    function setSlider(id, show) {
        var all = sliderList(), shown = QsLayout.toList(backend.sliders), out = [];
        for (var i = 0; i < all.length; i++)
            if (all[i] === id ? show : shown.indexOf(all[i]) >= 0) out.push(all[i]);
        backend.settings.sliders = out;
    }
    function moveSlider(id, delta) {
        var shown = QsLayout.toList(backend.sliders), i = shown.indexOf(id);
        if (i < 0 || i + delta < 0 || i + delta >= shown.length) return;
        shown.splice(i, 1);
        shown.splice(i + delta, 0, id);
        backend.settings.sliders = shown;
    }
    function sliderLabel(id) {
        return id === "brightness" ? i18n.tr("Brightness") : id === "volume" ? i18n.tr("Volume") : i18n.tr("Media controls");
    }

    // eat input underneath
    MouseArea { anchors.fill: parent }

    Item {
        id: topBar
        width: parent.width
        height: units.gu(6)
        AbstractButton {
            objectName: "qsEditDone"
            anchors { left: parent.left; leftMargin: units.gu(1); verticalCenter: parent.verticalCenter }
            width: doneLabel.width + units.gu(2); height: units.gu(5)
            onClicked: { editor.commit(); editor.done(); }
            Label { id: doneLabel; anchors.centerIn: parent; text: i18n.tr("Done"); font.weight: Font.Medium; color: theme.palette.normal.focus }
        }
        Label {
            anchors.centerIn: parent
            text: i18n.tr("Edit quick settings")
            font.weight: Font.Medium
            color: editor.fgColor
        }
        AbstractButton {
            objectName: "qsEditReset"
            anchors { right: parent.right; rightMargin: units.gu(1); verticalCenter: parent.verticalCenter }
            width: resetLabel.width + units.gu(2); height: units.gu(5)
            onClicked: { editor.backend.resetLayout(); Qt.callLater(editor.load); }
            Label { id: resetLabel; anchors.centerIn: parent; text: i18n.tr("Reset"); color: editor.fg2Color }
        }
    }

    Flickable {
        id: flick
        anchors { top: topBar.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        interactive: editor.dragId === ""
        clip: true

        Column {
            id: col
            x: units.gu(2)
            width: editor.width - units.gu(4)
            spacing: units.gu(1)

            Label {
                width: parent.width
                text: i18n.tr("Hold and drag to move. Drag down to remove. Tap to switch between small and wide.")
                textSize: Label.XSmall
                wrapMode: Text.Wrap
                color: editor.fg2Color
            }
            Item {
                id: activeArea
                width: parent.width
                height: Math.max(1, editor.activeLay.rows) * editor.cellHeight
                Behavior on height { LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
                // collapsed-rows marker
                Rectangle {
                    visible: editor.activeLay.rows > editor.backend.collapsedRows
                    y: editor.backend.collapsedRows * editor.cellHeight - height / 2
                    width: parent.width; height: units.dp(1)
                    color: editor.fg2Color; opacity: 0.5
                }
            }
            Item {
                id: trayHeader
                width: parent.width
                height: units.gu(5)
                Rectangle { width: parent.width; height: units.dp(1); color: editor.fg2Color; opacity: 0.3; anchors.verticalCenter: parent.verticalCenter }
                Label {
                    anchors.centerIn: parent
                    text: editor.trayIds.length ? i18n.tr("Drag to add tiles") : i18n.tr("All tiles are in use")
                    textSize: Label.XSmall
                    color: editor.fg2Color
                    Rectangle { anchors { fill: parent; margins: -units.gu(1) } z: -1; color: editor.lightMode ? "#F5F5F5" : "#1A1A1A"; radius: units.gu(1) }
                }
            }
            Item {
                id: trayArea
                width: parent.width
                height: Math.max(1, editor.trayLay.rows) * editor.cellHeight
            }

            // ---- layout options
            Label { text: i18n.tr("Layout"); textSize: Label.Small; font.weight: Font.Medium; color: editor.fgColor }
            Repeater {
                model: [
                    { key: "columns", label: i18n.tr("Tiles per row"), values: [4, 5, 6] },
                    { key: "collapsedRows", label: i18n.tr("Rows before expanding"), values: [1, 2, 3] }
                ]
                delegate: Item {
                    id: optRow
                    readonly property string key: modelData.key
                    readonly property int value: key === "columns" ? editor.backend.columns : editor.backend.collapsedRows
                    width: col.width
                    height: units.gu(5)
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: modelData.label
                        textSize: Label.Small
                        color: editor.fgColor
                    }
                    Row {
                        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                        spacing: units.gu(0.5)
                        Repeater {
                            model: modelData.values
                            delegate: AbstractButton {
                                readonly property bool current: modelData === optRow.value
                                objectName: "qsOpt-" + optRow.key + "-" + modelData
                                width: units.gu(4.5); height: units.gu(4)
                                onClicked: editor.backend.settings[optRow.key] = modelData
                                Rectangle {
                                    anchors.fill: parent
                                    radius: height / 2
                                    color: parent.current ? theme.palette.normal.focus : editor.cardColor
                                }
                                Label {
                                    anchors.centerIn: parent
                                    text: modelData
                                    color: parent.current ? theme.palette.normal.focusText : editor.fgColor
                                }
                            }
                        }
                    }
                }
            }

            // ---- sliders
            Label { text: i18n.tr("Sliders"); textSize: Label.Small; font.weight: Font.Medium; color: editor.fgColor }
            Repeater {
                model: editor.backend.sliders.length >= 0 ? editor.sliderList() : []
                delegate: Item {
                    readonly property bool shown: editor.backend.sliders.indexOf(modelData) >= 0
                    readonly property int pos: editor.backend.sliders.indexOf(modelData)
                    width: col.width
                    height: units.gu(5)
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: editor.sliderLabel(modelData)
                        textSize: Label.Small
                        color: editor.fgColor
                        opacity: parent.shown ? 1 : 0.6
                    }
                    Row {
                        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                        spacing: units.gu(0.5)
                        AbstractButton {
                            width: units.gu(4); height: units.gu(4)
                            enabled: parent.parent.shown && parent.parent.pos > 0
                            opacity: enabled ? 1 : 0.3
                            onClicked: editor.moveSlider(modelData, -1)
                            Icon { anchors.centerIn: parent; name: "go-up"; width: units.gu(2); height: width; color: editor.fgColor }
                        }
                        AbstractButton {
                            width: units.gu(4); height: units.gu(4)
                            enabled: parent.parent.shown && parent.parent.pos < editor.backend.sliders.length - 1
                            opacity: enabled ? 1 : 0.3
                            onClicked: editor.moveSlider(modelData, 1)
                            Icon { anchors.centerIn: parent; name: "go-down"; width: units.gu(2); height: width; color: editor.fgColor }
                        }
                        Switch {
                            objectName: "qsSliderSwitch-" + modelData
                            anchors.verticalCenter: parent.verticalCenter
                            checked: parent.parent.shown
                            onTriggered: editor.setSlider(modelData, checked)
                        }
                    }
                }
            }
        }

        // ---- the tiles (active and tray), one delegate per catalogue entry
        Repeater {
            model: editor.backend.catalogue
            delegate: Item {
                id: et
                readonly property string tid: modelData.tileId
                readonly property var ap: editor.activeLay.pos[tid]
                readonly property var tp: editor.trayLay.pos[tid]
                readonly property bool inActive: !!ap
                readonly property bool dragging: editor.dragId === tid
                width: inActive ? ap.w : editor.cellWidth
                height: editor.cellHeight
                x: dragging ? editor.dragX - width / 2
                   : col.x + (inActive ? ap.x : (tp ? tp.x : 0))
                y: dragging ? editor.dragY - height / 2
                   : col.y + (inActive ? activeArea.y + ap.y : trayArea.y + (tp ? tp.y : 0))
                z: dragging ? 10 : 1
                Behavior on x { enabled: !et.dragging; LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
                Behavior on y { enabled: !et.dragging; LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }

                QsTile {
                    anchors.fill: parent
                    def: modelData
                    wide: et.inActive && et.ap.span > 1
                    diameter: editor.diameter
                    labelLines: QsLayout.labelLines(editor.columns)
                    lightMode: editor.lightMode
                    editMode: true
                    scale: et.dragging ? 1.08 : 1
                    Behavior on scale { LomiriNumberAnimation { duration: LomiriAnimation.SnapDuration } }
                }
                // -/+ badge
                AbstractButton {
                    objectName: "qsBadge-" + et.tid
                    width: units.gu(3); height: width
                    x: et.inActive && et.ap.span > 1 ? et.width - width : (et.width + editor.diameter) / 2 - width * 0.7
                    y: 0
                    visible: !et.dragging
                    onClicked: et.inActive ? editor.remove(et.tid) : editor.add(et.tid)
                    Rectangle {
                        anchors.centerIn: parent
                        width: units.gu(2.25); height: width; radius: width / 2
                        color: et.inActive ? (editor.lightMode ? "#5D5D5D" : "#CDCDCD") : theme.palette.normal.focus
                        Label {
                            anchors.centerIn: parent
                            text: et.inActive ? "−" : "+"
                            font.weight: Font.Bold
                            color: et.inActive ? (editor.lightMode ? "#FFFFFF" : "#111111") : theme.palette.normal.focusText
                        }
                    }
                }
                MouseArea {
                    anchors { fill: parent; topMargin: units.gu(2.5) }
                    preventStealing: et.dragging
                    pressAndHoldInterval: 350
                    onPressAndHold: editor.beginDrag(et.tid, mapToItem(flick.contentItem, mouse.x, mouse.y))
                    onPositionChanged: if (et.dragging) editor.moveDrag(mapToItem(flick.contentItem, mouse.x, mouse.y))
                    onReleased: if (et.dragging) editor.endDrag()
                    onCanceled: if (et.dragging) editor.endDrag()
                    onClicked: {
                        if (et.inActive) editor.toggleWide(et.tid);
                        else editor.add(et.tid);
                    }
                }
            }
        }
    }
}
