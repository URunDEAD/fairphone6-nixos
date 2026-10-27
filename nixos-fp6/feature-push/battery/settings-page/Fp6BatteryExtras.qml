/*
 * FP6 rows in System Settings > Battery (loaded by the stock page right
 * after "Charge level"/"Charging now"): charging speed and time to full,
 * Battery saver, Charge limit / adaptive charging, Battery health and
 * history, Battery usage.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

Column {
    id: extras
    property var page: null
    readonly property alias m: model

    Fp6BatteryModel { id: model }

    function open(file) {
        pageStack.addPageToNextColumn(page, Qt.resolvedUrl(file), { m: model })
    }

    // Long text: a wrapped label instead of a SingleValue row (it overflowed).
    Label {
        objectName: "fp6ChargingSpeed"
        visible: model.plugged
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2); topMargin: units.gu(1); bottomMargin: units.gu(1) }
        wrapMode: Text.WordWrap
        text: model.chargingText()
    }

    SettingsListItems.SingleValue {
        objectName: "fp6TimeToFull"
        visible: model.plugged && model.fullText() !== ""
        text: model.limit && model.limit.mode === "hold" ? i18n.tr("Adaptive charging") : i18n.tr("Charge status")
        value: model.fullText()
        showDivider: false
    }

    SettingsItemTitle { text: i18n.tr("Battery protection and saving") }

    SettingsListItems.SingleValueProgression {
        objectName: "fp6BatterySaver"
        text: i18n.tr("Battery saver")
        value: model.saver && model.saver.active ? (model.saver.auto ? i18n.tr("On (low battery)") : i18n.tr("On"))
                                                    : i18n.tr("Off")
        onClicked: extras.open("Fp6SaverPage.qml")
    }

    SettingsListItems.SingleValueProgression {
        objectName: "fp6ChargeLimit"
        text: i18n.tr("Charge limit")
        value: {
                var l = model.settings.chargeLimit
                var s = l >= 100 ? i18n.tr("Off") : l + " %"
                if (model.settings.adaptiveCharging) s += i18n.tr(", adaptive")
                return s
            }
        onClicked: extras.open("Fp6ChargingPage.qml")
    }

    SettingsListItems.SingleValueProgression {
        objectName: "fp6BatteryHealth"
        text: i18n.tr("Battery health and history")
        value: model.health && model.health.percent > 0 ? model.health.percent + " %" : ""
        onClicked: extras.open("Fp6HealthPage.qml")
    }

    SettingsListItems.StandardProgression {
        objectName: "fp6BatteryUsage"
        text: i18n.tr("Battery usage")
        onClicked: extras.open("Fp6UsagePage.qml")
    }

    SettingsListItems.Standard {
        text: i18n.tr("Show charging speed in the status bar")
        CheckBox {
            objectName: "fp6ChargingIndicator"
            SlotsLayout.position: SlotsLayout.First
            property bool serverChecked: model.settings.chargingIndicator
            onServerCheckedChanged: checked = serverChecked
            Component.onCompleted: checked = serverChecked
            onTriggered: model.settings.chargingIndicator = checked
        }
    }
}
