/*
 * FP6 quick settings: the media players of the sound indicator, drawn with
 * the indicator page's own menu items (read only: the Output/Input sections
 * of the sound menu are not shown here).
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import Lomiri.Indicators 0.1 as Indicators
import "../Indicators"

Column {
    id: media
    property var info: null
    property bool shown: false
    readonly property bool hasPlayers: d.count > 0

    QtObject { id: d; property int count: 0 }

    Loader {
        width: parent.width
        active: media.shown && !!media.info && !!media.info.busName
        sourceComponent: Column {
            width: parent ? parent.width : 0
            Indicators.SharedLomiriMenuModel {
                id: cached
                busName: media.info.busName
                actions: { "indicator": media.info.actionsObjectPath }
                menuObjectPath: media.info.menuObjectPath
            }
            readonly property var sub: cached.model ? cached.model.submenu(0) : null
            IndicatorMenuItemFactory {
                id: factory
                indicator: "ayatana-indicator-sound"
                rootModel: cached.model
                menuModel: parent.sub
            }
            Repeater {
                model: parent.sub
                delegate: Loader {
                    readonly property bool isMedia: /media-player|playback-item/.test(model.type || "")
                    property int modelIndex: index
                    width: media.width
                    active: isMedia
                    visible: isMedia && status == Loader.Ready
                    sourceComponent: isMedia ? factory.load(model) : null
                    onLoaded: {
                        if (item.hasOwnProperty("menuData")) item.menuData = Qt.binding(function() { return model; });
                        if (item.hasOwnProperty("menuIndex")) item.menuIndex = Qt.binding(function() { return modelIndex; });
                    }
                    Component.onCompleted: if (isMedia) d.count++
                    Component.onDestruction: if (isMedia) d.count--
                }
            }
        }
    }
}
