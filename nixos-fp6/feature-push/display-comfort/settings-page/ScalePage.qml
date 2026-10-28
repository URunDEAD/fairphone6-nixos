/*
 * Display Comfort > Text and display size: interface scale (the shell's grid
 * unit; Mir output scale = GU / 8, next login) and text size (live in the
 * shell and Lomiri apps through the font-scale UITK, GTK live, other Qt apps
 * at the next login), bold text (UITK default weight, next login), with
 * presets and a preview that follows the slider and the bold switch.
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

    // The text size this process draws with right now (1 with the stock UITK;
    // the font-scale UITK re-evaluates this when the live size changes).
    readonly property real appliedFont: FontUtils.sizeToPixels("medium") / units.dp(14)
    // The UITK text size follows font-scale live only with the font-scale
    // library loaded (Display Comfort fontScaleUitk).
    readonly property bool liveText: Math.abs(appliedFont - gs.fontScale / 100) < 0.01
    // Preview: the slider value (also while dragging) and the chosen display
    // size, relative to what this page is drawn with.
    readonly property real sizeRatio: gs.interfaceScale / Math.max(1, gs.sessionInterfaceScale)
    readonly property real fontRatio: (fontSlider.value / 100) / Math.max(0.5, appliedFont)
    readonly property bool sizePending: gs.interfaceScale !== gs.sessionInterfaceScale
    // Point-sized Qt fonts (QT_WAYLAND_FORCE_DPI) are fixed at login.
    readonly property bool fontPending: gs.fontScale !== gs.sessionFontScale
    // Bold text: UITK default weight Light -> Normal, fixed at login.
    readonly property bool boldPending: gs.boldText !== gs.sessionBoldText
    readonly property int previewWeight: gs.boldText ? Font.Normal : Font.Light

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
                // Grows with the previewed text, so the largest size is not clipped.
                height: (previewContent.height + units.gu(3)) * page.sizeRatio
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
                        id: previewContent
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                        spacing: units.gu(1)
                        Row {
                            spacing: units.gu(1.5)
                            Rectangle { width: units.gu(5); height: width; radius: width / 4; color: LomiriColors.orange }
                            Column {
                                anchors.verticalCenter: parent.verticalCenter
                                Label { text: i18n.tr("Messages"); font.pixelSize: FontUtils.sizeToPixels("medium") * page.fontRatio; font.weight: page.previewWeight }
                                Label { text: i18n.tr("3 new messages"); font.pixelSize: FontUtils.sizeToPixels("small") * page.fontRatio; font.weight: page.previewWeight; color: theme.palette.normal.backgroundSecondaryText }
                            }
                        }
                        Label {
                            width: parent.width
                            wrapMode: Text.WordWrap
                            maximumLineCount: 2
                            elide: Text.ElideRight
                            text: i18n.tr("The quick brown fox jumps over the lazy dog.")
                            font.pixelSize: FontUtils.sizeToPixels("medium") * page.fontRatio
                            font.weight: page.previewWeight
                        }
                        Rectangle {
                            width: Math.max(units.gu(14), previewButtonLabel.width + units.gu(4))
                            height: Math.max(units.gu(4), previewButtonLabel.height + units.gu(1.5))
                            radius: units.gu(0.7)
                            color: theme.palette.normal.positive
                            Label { id: previewButtonLabel; anchors.centerIn: parent; text: i18n.tr("Button"); color: "white"; font.pixelSize: FontUtils.sizeToPixels("medium") * page.fontRatio; font.weight: page.previewWeight }
                        }
                    }
                }
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Display size") }
            OptionSelector {
                objectName: "displaySizeSelector"
                // Always expanded: the collapsed selector could show the wrong
                // row at start (seen with Default), and a list of five stays
                // readable at the largest text size.
                expanded: true
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
            // Four presets in a row, or 2 x 2 when the longest label would not
            // fit (large text sizes, long translations).
            TextMetrics {
                id: presetMetrics
                font.pixelSize: FontUtils.sizeToPixels("medium")
                text: page.fontPresetLabels.reduce(function (a, b) { return a.length >= b.length ? a : b })
            }
            Grid {
                id: presetGrid
                objectName: "fontPresetGrid"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(1)
                columns: presetMetrics.advanceWidth + units.gu(3) <= (width - 3 * spacing) / 4 ? 4 : 2
                Repeater {
                    model: page.fontPresets.length
                    Button {
                        objectName: "fontPreset" + index
                        width: (presetGrid.width - (presetGrid.columns - 1) * presetGrid.spacing) / presetGrid.columns
                        text: page.fontPresetLabels[index]
                        color: gs.fontScale === page.fontPresets[index] ? theme.palette.normal.focus : theme.palette.normal.base
                        onClicked: gs.fontScale = page.fontPresets[index]
                    }
                }
            }
            Slider {
                id: fontSlider
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
                text: page.liveText
                    ? i18n.tr("Changes right away in the shell, Lomiri apps and GTK apps. Other Qt apps and Firefox follow when they are restarted or at the next login.")
                    : i18n.tr("Lomiri apps and the shell, Qt apps, GTK apps and Firefox, at the next login.")
            }

            ListItem.ThinDivider {}
            SettingSwitch {
                objectName: "boldText"
                text: i18n.tr("Bold text")
                settings: gs
                key: "boldText"
            }
            Note {
                objectName: "boldNote"
                font.weight: page.boldPending ? Font.Medium : Qt.application.font.weight
                text: page.boldPending
                    ? i18n.tr("Bold text takes effect the next time you log in (Log Out in the power menu, or restart the phone). The preview above shows it already.")
                    : i18n.tr("Heavier text in the shell and Lomiri apps, at the next login. Text that an app already draws bold or in a fixed weight stays as it is.")
            }

            ListItem.ThinDivider {}
            Note {
                objectName: "pendingNote"
                font.weight: (page.sizePending || page.fontPending) ? Font.Medium : Qt.application.font.weight
                text: page.sizePending
                    ? i18n.tr("The display size takes effect the next time you log in (Log Out in the power menu, or restart the phone).")
                    : page.fontPending
                    ? i18n.tr("Some apps (Qt apps outside Lomiri) show the new text size after the next login.")
                    : i18n.tr("Display size changes take effect at the next login.")
            }
            Button {
                objectName: "resetScale"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: gs.interfaceScale !== 24 || gs.fontScale !== 100 || gs.boldText
                text: i18n.tr("Reset to default")
                onClicked: { gs.interfaceScale = 24; gs.fontScale = 100; gs.boldText = false }
            }
        }
    }
}
