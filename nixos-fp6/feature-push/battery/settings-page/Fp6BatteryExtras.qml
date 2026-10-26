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

    SettingsListItems.SingleValue {
        objectName: "fp6ChargingSpeed"
        visible: model.plugged
        text: i18n.tr("Charging")
        value: model.chargingText()
        showDivider: false
    }

    SettingsListItems.SingleValue {
        objectName: "fp6TimeToFull"
        visible: model.plugged && model.fullText() !== ""
        text: " "
        value: model.fullText()
        showDivider: false
    }

    SettingsItemTitle { text: i18n.tr("Battery protection and saving") }

    SettingsListItems.StandardProgression {
        objectName: "fp6BatterySaver"
        text: i18n.tr("Battery saver")
        Label {
            SlotsLayout.position: SlotsLayout.Trailing
            text: model.saver && model.saver.active ? (model.saver.auto ? i18n.tr("On (low battery)") : i18n.tr("On"))
                                                    : i18n.tr("Off")
        }
        onClicked: extras.open("Fp6SaverPage.qml")
    }

    SettingsListItems.StandardProgression {
        objectName: "fp6ChargeLimit"
        text: i18n.tr("Charge limit")
        Label {
            SlotsLayout.position: SlotsLayout.Trailing
            text: {
                var l = model.settings.chargeLimit
                var s = l >= 100 ? i18n.tr("Off") : l + " %"
                if (model.settings.adaptiveCharging) s += i18n.tr(", adaptive")
                return s
            }
        }
        onClicked: extras.open("Fp6ChargingPage.qml")
    }

    SettingsListItems.StandardProgression {
        objectName: "fp6BatteryHealth"
        text: i18n.tr("Battery health and history")
        Label {
            SlotsLayout.position: SlotsLayout.Trailing
            text: model.health && model.health.percent > 0 ? model.health.percent + " %" : ""
        }
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
