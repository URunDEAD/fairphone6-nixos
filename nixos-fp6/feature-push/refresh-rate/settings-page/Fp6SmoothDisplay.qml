/*
 * System Settings > Brightness & Display: "Smooth display" (refresh rate).
 * Loaded by the stock brightness PageComponent.qml (lss-brightness-
 * smooth-display.patch). Backend: GSettings com.fairphone6.refresh-rate,
 * applied by Lomiri (Fp6RefreshRate.qml). A mode that was never kept
 * before runs on trial: "Keep" within 15 s, else it goes back.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import Lomiri.Components.Popups 1.3
import GSettings 1.0
import "Fp6RefreshRate.js" as RR

Column {
    id: smooth
    objectName: "fp6SmoothDisplay"

    GSettings {
        id: rr
        schema.id: "com.fairphone6.refresh-rate"
    }
    GSettings {
        id: battery
        schema.id: "com.fairphone6.battery"
    }

    readonly property bool schemaOk: rr.mode !== undefined
    readonly property var rates: RR.parseRates(rr.availableRates)
    readonly property bool adaptiveOk: rr.adaptiveAvailable === true
    readonly property bool saver: battery.saverActive === true

    // [mode, label] in display order, only what this kernel offers
    readonly property var options: {
        var o = [];
        if (adaptiveOk)
            o.push(["adaptive", i18n.tr("Adaptive (120 Hz when touched, lower when idle)")]);
        if (rates.indexOf(120) >= 0)
            o.push(["high", i18n.tr("Smooth (120 Hz)")]);
        if (rates.indexOf(90) >= 0)
            o.push(["medium", i18n.tr("Balanced (90 Hz)")]);
        if (rates.indexOf(60) >= 0)
            o.push(["standard", i18n.tr("Standard (60 Hz)")]);
        if (rates.indexOf(30) >= 0)
            o.push(["low", i18n.tr("Power saving (30 Hz)")]);
        return o;
    }
    readonly property bool switchable: options.length > 1
    function optionIndex(mode) {
        for (var i = 0; i < options.length; i++)
            if (options[i][0] === mode)
                return i;
        return -1;
    }

    // trial of a mode that was never kept
    property string previousMode: ""
    property int countdown: 0
    property var dialog: null

    function choose(mode) {
        if (mode === rr.mode)
            return;
        if (mode !== "low" && mode !== "standard")
            rr.smoothMode = mode;
        if (RR.listHas(rr.confirmedModes, mode)) {
            rr.mode = mode;
            return;
        }
        previousMode = rr.mode;
        rr.confirmed = false;
        rr.mode = mode;
        countdown = 15;
        trialTimer.start();
        dialog = PopupUtils.open(confirmDialog, smooth);
    }
    function keep() {
        trialTimer.stop();
        rr.confirmedModes = RR.listAdd(rr.confirmedModes, rr.mode);
        rr.confirmed = true;
        closeDialog();
    }
    function revert() {
        trialTimer.stop();
        if (previousMode !== "")
            rr.mode = previousMode;
        rr.confirmed = true;
        closeDialog();
    }
    function closeDialog() {
        if (dialog)
            PopupUtils.close(dialog);
        dialog = null;
    }

    Timer {
        id: trialTimer
        interval: 1000
        repeat: true
        onTriggered: {
            smooth.countdown--;
            if (smooth.countdown <= 0)
                smooth.revert();
        }
    }

    Component {
        id: confirmDialog
        Dialog {
            id: dlg
            objectName: "fp6SmoothDisplayConfirm"
            title: i18n.tr("Keep this refresh rate?")
            text: i18n.tr("Going back in %1 s.").arg(smooth.countdown)
            Button {
                objectName: "keep"
                text: i18n.tr("Keep")
                color: theme.palette.normal.positive
                onClicked: smooth.keep()
            }
            Button {
                objectName: "revert"
                text: i18n.tr("Go back")
                onClicked: smooth.revert()
            }
        }
    }

    ListItem.Divider { visible: smooth.schemaOk }

    SettingsItemTitle {
        text: i18n.tr("Smooth display:")
        visible: smooth.schemaOk
    }

    ListItem.ItemSelector {
        id: selector
        objectName: "fp6RefreshRateSelector"
        visible: smooth.schemaOk && smooth.switchable
        expanded: true
        model: smooth.options.map(function (o) { return o[1]; })
        selectedIndex: Math.max(0, smooth.optionIndex(rr.mode))
        onDelegateClicked: smooth.choose(smooth.options[index][0])
    }

    ListItem.ItemSelector {
        id: idleSelector
        objectName: "fp6AdaptiveIdleRate"
        visible: smooth.schemaOk && smooth.adaptiveOk && rr.mode === "adaptive"
        text: i18n.tr("When idle")
        readonly property var values: [60, 30, 10]
        model: [i18n.tr("60 Hz (smooth video)"), i18n.tr("30 Hz"), i18n.tr("10 Hz (still screens)")]
        selectedIndex: Math.max(0, values.indexOf(rr.adaptiveIdleRate))
        onDelegateClicked: rr.adaptiveIdleRate = values[index]
    }

    ListItem.Standard {
        objectName: "fp6RefreshSaverCap"
        visible: smooth.schemaOk && smooth.switchable
        text: i18n.tr("Battery saver limits to 60 Hz")
        control: Switch {
            property bool serverChecked: rr.saverCap !== false
            onServerCheckedChanged: checked = serverChecked
            Component.onCompleted: checked = serverChecked
            onTriggered: rr.saverCap = checked
        }
        showDivider: false
    }

    ListItem.Caption {
        visible: smooth.schemaOk
        text: {
            if (rr.availableRates === "" || rr.availableRates === undefined)
                return i18n.tr("The display has not reported its refresh rates yet.");
            if (!smooth.switchable)
                return i18n.tr("This system offers only %1 Hz.").arg(smooth.rates.join(", "));
            var s = rr.activeRate > 0 ? i18n.tr("Now %1 Hz.").arg(rr.activeRate) + " " : "";
            if (smooth.saver && rr.saverCap !== false)
                s += i18n.tr("Battery saver is on: at most 60 Hz.") + " ";
            return s + i18n.tr("Higher rates look smoother but use more battery while the screen moves. Changing a fixed rate blanks the screen for a moment.");
        }
    }
}
