/*
 * FP6 quick settings: the tile grid. `collapsedRows` rows show until it is
 * expanded (chevron below it, or a downward swipe on the grid).
 * One delegate per catalogue entry, so reordering only moves tiles.
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import "QsLayout.js" as QsLayout

Item {
    id: grid
    property QtObject backend
    property bool lightMode: false
    property bool locked: false
    property bool expanded: false
    signal tileTapped(QtObject def)
    signal tileDetail(QtObject def)

    readonly property int columns: backend.columns
    readonly property real cellWidth: width / columns
    readonly property real diameter: Math.min(cellWidth - units.gu(2), units.gu(6.5))
    readonly property real cellHeight: diameter + units.gu(QsLayout.labelLines(columns) > 1 ? 5 : 3.5)

    readonly property var visibleIds: {
        var out = [], list = backend.tiles;
        for (var i = 0; i < list.length; i++) {
            var t = backend.tile(list[i]);
            if (t && t.available) out.push(list[i]);
        }
        return out;
    }
    readonly property var lay: QsLayout.layout(visibleIds, backend.wideTiles, columns, cellWidth, cellHeight)
    readonly property int shownRows: expanded ? lay.rows : Math.min(lay.rows, backend.collapsedRows)
    readonly property bool canExpand: lay.rows > backend.collapsedRows

    height: shownRows * cellHeight
    Behavior on height { LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
    clip: true

    Repeater {
        model: grid.backend.catalogue
        delegate: QsTile {
            readonly property var p: grid.lay.pos[modelData.tileId]
            def: modelData
            visible: !!p
            wide: !!p && p.span > 1
            x: p ? p.x : 0
            y: p ? p.y : 0
            width: p ? p.w : grid.cellWidth
            height: grid.cellHeight
            diameter: grid.diameter
            labelLines: QsLayout.labelLines(grid.columns)
            lightMode: grid.lightMode
            locked: grid.locked
            opacity: p && p.row < grid.shownRows ? 1 : 0
            Behavior on opacity { LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
            Behavior on x { enabled: grid.visible; LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
            Behavior on y { enabled: grid.visible; LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
            onTapped: grid.tileTapped(def)
            onHeld: grid.tileDetail(def)
            onDetailRequested: grid.tileDetail(def)
        }
    }
}
