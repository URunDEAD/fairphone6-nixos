/*
 * Display Comfort > Text and display size: interface scale (the shell's grid
 * unit; Mir output scale = GU / 8) and text size, applied at the next login,
 * with presets and a preview.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems
import GSettings 1.0

ItemPage {
    id: page
    objectName: "scalePage"
    title: i18n.tr("Text and Display Size")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.display-comfort"
    }

    readonly property var scales: [20, 22, 24, 26, 28]
    readonly property var scaleLabels: [i18n.tr("Smaller (83 %)"), i18n.tr("Small (92 %)"), i18n.tr("Default"),
                                        i18n.tr("Large (108 %)"), i18n.tr("Larger (117 %)")]
    readonly property var fontPresets: [90, 100, 115, 130]
    readonly property var fontPresetLabels: [i18n.tr("Small"), i18n.tr("Default"), i18n.tr("Large"), i18n.tr("Largest")]

    // Pending values relative to what this session runs with.
    readonly property real sizeRatio: gs.interfaceScale / Math.max(1, gs.sessionInterfaceScale)
    readonly property real fontRatio: gs.fontScale / Math.max(1, gs.sessionFontScale)
    readonly property bool pending: gs.interfaceScale !== gs.sessionInterfaceScale || gs.fontScale !== gs.sessionFontScale

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > page.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: units.gu(1)

            SettingsItemTitle { text: i18n.tr("Preview") }
            // A list item, a button and body text drawn at the chosen sizes.
            Rectangle {
                id: preview
                objectName: "scalePreview"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                height: units.gu(19)
                radius: units.gu(1)
                clip: true
                color: theme.palette.normal.background
                border.color: theme.palette.normal.base

                Item {
                    id: scaled
                    width: preview.width / page.sizeRatio
                    height: preview.height / page.sizeRatio
                    transform: Scale { xScale: page.sizeRatio; yScale: page.sizeRatio }
                    Column {
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                        spacing: units.gu(1)
                        Row {
                            spacing: units.gu(1.5)
                            Rectangle { width: units.gu(5); height: width; radius: width / 4; color: LomiriColors.orange }
                            Column {
                                anchors.verticalCenter: parent.verticalCenter
                                Label { text: i18n.tr("Messages"); font.pixelSize: FontUtils.sizeToPixels("medium") * page.fontRatio }
                                Label { text: i18n.tr("3 new messages"); font.pixelSize: FontUtils.sizeToPixels("small") * page.fontRatio; color: theme.palette.normal.backgroundSecondaryText }
                            }
                        }
                        Label {
                            width: parent.width
                            wrapMode: Text.WordWrap
                            maximumLineCount: 2
                            elide: Text.ElideRight
                            text: i18n.tr("The quick brown fox jumps over the lazy dog.")
                            font.pixelSize: FontUtils.sizeToPixels("medium") * page.fontRatio
                        }
                        Rectangle {
                            width: units.gu(14); height: units.gu(4); radius: units.gu(0.7)
                            color: theme.palette.normal.positive
                            Label { anchors.centerIn: parent; text: i18n.tr("Button"); color: "white"; font.pixelSize: FontUtils.sizeToPixels("medium") * page.fontRatio }
                        }
                    }
                }
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Display size") }
            OptionSelector {
                objectName: "displaySizeSelector"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                model: page.scaleLabels
                selectedIndex: page.scales.indexOf(gs.interfaceScale) >= 0 ? page.scales.indexOf(gs.interfaceScale) : 2
                onDelegateClicked: gs.interfaceScale = page.scales[index]
            }
            Note {
                text: i18n.tr("Everything (shell and apps) gets bigger or smaller. Sizes other than Default are drawn at a higher resolution and scaled, so text can look slightly softer.")
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Text size") }
            Row {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(1)
                Repeater {
                    model: page.fontPresets.length
                    Button {
                        objectName: "fontPreset" + index
                        width: (parent.width - 3 * units.gu(1)) / 4
                        text: page.fontPresetLabels[index]
                        color: gs.fontScale === page.fontPresets[index] ? theme.palette.normal.focus : theme.palette.normal.base
                        onClicked: gs.fontScale = page.fontPresets[index]
                    }
                }
            }
            Slider {
                objectName: "fontSlider"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                minimumValue: 80
                maximumValue: 150
                stepSize: 5
                live: true
                function formatValue(v) { return i18n.tr("%1 %").arg(Math.round(v)) }
                property int serverValue: gs.fontScale
                onServerValueChanged: if (!pressed) value = serverValue
                Component.onCompleted: value = serverValue
                onPressedChanged: if (!pressed) gs.fontScale = Math.round(value / 5) * 5
            }
            Note {
                text: i18n.tr("Lomiri apps and the shell, Qt apps, GTK apps and Firefox.")
            }

            ListItem.ThinDivider {}
            Note {
                objectName: "pendingNote"
                font.weight: page.pending ? Font.Medium : Font.Normal
                text: page.pending
                    ? i18n.tr("Takes effect the next time you log in (Log Out in the power menu, or restart the phone).")
                    : i18n.tr("Changes take effect at the next login.")
            }
            Button {
                objectName: "resetScale"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: gs.interfaceScale !== 24 || gs.fontScale !== 100
                text: i18n.tr("Reset to default")
                onClicked: { gs.interfaceScale = 24; gs.fontScale = 100 }
            }
        }
    }
}
