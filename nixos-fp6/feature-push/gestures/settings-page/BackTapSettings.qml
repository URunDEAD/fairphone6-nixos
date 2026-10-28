/*
 * FP6 gesture shortcuts: back tap sensitivity, guided calibration and a
 * live test mode. fp6-gestures does the detection: the page sets
 * back-tap-mode (normal / calibrate / test) and back-tap-level, and shows
 * back-tap-cal-progress, back-tap-log and back-tap-profile.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

Column {
    id: bt
    objectName: "backTapSettings"

    property var settings
    readonly property string mode: settings ? settings.backTapMode : "normal"
    readonly property int progress: settings ? settings.backTapCalProgress : 0
    // backtap::CAL_GROUPS
    readonly property int pairs: 4
    readonly property var profile: parseProfile(settings ? settings.backTapProfile : "")
    readonly property bool calibrated: profile.peak !== undefined
    // back-tap-log, newest first.
    readonly property var entries: parseLog(settings ? settings.backTapLog : [])
    // The calibration that just ended (the daemon keeps it in the log).
    readonly property var result: (mode === "normal" && entries.length > 0
                                   && (entries[0].kind === "calibrated" || entries[0].kind === "failed")) ? entries[0] : null
    readonly property var lastTap: {
        for (var i = 0; i < entries.length; i++)
            if (entries[i].kind === "tap") return entries[i]
        return null
    }

    // "v1 peak=0.912 min=0.700 … sign=-1 n=8" -> {peak: 0.912, …}; {} if empty.
    function parseProfile(text) {
        var out = {}
        var parts = String(text || "").split(" ")
        if (parts[0] !== "v1") return out
        for (var i = 1; i < parts.length; i++) {
            var kv = parts[i].split("=")
            if (kv.length === 2) out[kv[0]] = parseFloat(kv[1])
        }
        return out
    }

    // "seq TAB kind TAB a TAB b" entries, newest first.
    function parseLog(list) {
        var out = []
        for (var i = 0; i < (list ? list.length : 0); i++) {
            var p = String(list[i]).split("\t")
            out.unshift({ seq: p[0], kind: p[1] || "", a: p[2] || "", b: p[3] || "" })
        }
        return out
    }

    function g(v) {
        return Number(v).toLocaleString(Qt.locale(), "f", 2)
    }

    function whyText(code) {
        switch (code) {
        case "light": return i18n.tr("too light")
        case "sideways": return i18n.tr("not on the back (a knock on the side)")
        case "close": return i18n.tr("too close to the previous tap")
        case "cooldown": return i18n.tr("right after a gesture")
        case "flat": return i18n.tr("lying flat: a knock on the table")
        case "touch": return i18n.tr("the screen was touched")
        case "typing": return i18n.tr("you were using the screen")
        case "side": return i18n.tr("it came from the screen side")
        case "burst": return i18n.tr("too many taps in a row")
        }
        return code
    }

    function calErrorText(code) {
        switch (code) {
        case "light": return i18n.tr("The taps were too light to tell apart from handling the phone. Tap a little more firmly.")
        case "uneven": return i18n.tr("The taps were very different in strength. Try to tap the same way each time.")
        case "timing": return i18n.tr("The two taps of a pair were too far apart or too close. Tap twice quickly, like a double click.")
        case "timeout": return i18n.tr("Not enough pairs were recorded in time.")
        }
        return code
    }

    function describe(e) {
        switch (e.kind) {
        case "tap": return i18n.tr("Tap, %1 g (needs %2 g)").arg(g(e.a)).arg(g(e.b))
        case "ignored": return i18n.tr("Ignored impulse, %1 g: %2").arg(g(e.b)).arg(whyText(e.a))
        case "moving": return i18n.tr("Moving: taps are ignored until the phone is held still")
        case "gesture": return e.a === "3" ? i18n.tr("Triple tap recognised") : i18n.tr("Double tap recognised")
        case "rejected": return i18n.tr("%1 taps not counted: %2").arg(e.b).arg(whyText(e.a))
        case "calibrated": return i18n.tr("Calibrated")
        case "failed": return i18n.tr("Calibration failed")
        }
        return e.kind
    }

    function kindColor(kind) {
        if (kind === "gesture" || kind === "calibrated") return theme.palette.normal.positive
        if (kind === "tap") return theme.palette.normal.activity
        if (kind === "rejected" || kind === "failed") return theme.palette.normal.negative
        return theme.palette.normal.base
    }

    spacing: units.gu(1)
    anchors.left: parent.left
    anchors.right: parent.right

    // ---------------------------------------------------------- sensitivity
    Column {
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        spacing: units.gu(0.5)
        Label { text: i18n.tr("Sensitivity"); font.weight: Font.Medium }
        Slider {
            id: slider
            objectName: "backTapLevelSlider"
            width: parent.width
            minimumValue: 0
            maximumValue: 100
            live: false
            function formatValue(v) { return Math.round(v) }
            Component.onCompleted: value = bt.settings ? bt.settings.backTapLevel : 50
            onValueChanged: {
                var v = Math.round(value)
                if (bt.settings && v !== bt.settings.backTapLevel) bt.settings.backTapLevel = v
            }
            Connections {
                target: bt.settings
                ignoreUnknownSignals: true
                function onBackTapLevelChanged() {
                    if (!slider.pressed) slider.value = bt.settings.backTapLevel
                }
            }
        }
        Item {
            width: parent.width
            height: firm.height
            Label { id: firm; anchors.left: parent.left; fontSize: "small"; text: i18n.tr("Firm taps only") }
            Label { anchors.right: parent.right; fontSize: "small"; text: i18n.tr("Light taps") }
        }
    }

    Label {
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        wrapMode: Text.WordWrap
        fontSize: "small"
        text: bt.calibrated
            ? i18n.tr("Calibrated to your taps: a typical tap is %1 g, the two taps of a pair up to %2 s apart. The slider moves the threshold from twice to half of what the calibration chose.")
                  .arg(g(bt.profile.peak)).arg(g(bt.profile.gapmax))
            : i18n.tr("Not calibrated: the default threshold is used. Calibrating takes half a minute and makes back tap fit how you hold and tap the phone.")
    }

    // ---------------------------------------------------------- calibration
    SettingsListItems.Standard {
        text: bt.mode === "calibrate" ? i18n.tr("Calibrating…")
              : bt.calibrated ? i18n.tr("Calibrate again") : i18n.tr("Calibrate for your taps")
        showDivider: false
        Button {
            objectName: "backTapCalibrateButton"
            text: bt.mode === "calibrate" ? i18n.tr("Cancel") : i18n.tr("Start")
            color: bt.mode === "calibrate" ? theme.palette.normal.base : theme.palette.normal.positive
            onClicked: bt.settings.backTapMode = (bt.mode === "calibrate" ? "normal" : "calibrate")
        }
    }

    Column {
        objectName: "backTapCalibrationPanel"
        visible: bt.mode === "calibrate"
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        spacing: units.gu(1.5)

        Label {
            width: parent.width
            wrapMode: Text.WordWrap
            text: i18n.tr("Hold the phone the way you usually do, keep your fingers off the screen, and tap the back twice quickly, like a double click. Wait a second and repeat until all circles are filled.")
        }
        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: units.gu(2)
            Repeater {
                model: bt.pairs
                Rectangle {
                    width: units.gu(4)
                    height: width
                    radius: width / 2
                    color: index < bt.progress ? theme.palette.normal.positive : "transparent"
                    border.width: units.dp(2)
                    border.color: index < bt.progress ? theme.palette.normal.positive : theme.palette.normal.base
                }
            }
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            fontSize: "small"
            text: i18n.tr("%1 of %2 pairs").arg(bt.progress).arg(bt.pairs)
                  + (bt.lastTap ? "  ·  " + i18n.tr("last tap %1 g").arg(g(bt.lastTap.a)) : "")
        }
    }

    Label {
        objectName: "backTapCalibrationResult"
        visible: bt.result !== null
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        wrapMode: Text.WordWrap
        color: bt.result && bt.result.kind === "failed" ? theme.palette.normal.negative : theme.palette.normal.positive
        text: !bt.result ? ""
              : bt.result.kind === "calibrated" ? i18n.tr("Done. Try it below with the test mode.")
              : calErrorText(bt.result.a)
    }

    SettingsListItems.Standard {
        visible: bt.calibrated && bt.mode === "normal"
        text: i18n.tr("Forget the calibration")
        showDivider: false
        Button {
            objectName: "backTapResetButton"
            text: i18n.tr("Reset")
            onClicked: bt.settings.backTapProfile = ""
        }
    }

    // ------------------------------------------------------------- test mode
    SettingsListItems.Standard {
        text: i18n.tr("Test mode")
        showDivider: false
        Switch {
            objectName: "backTapTestSwitch"
            property bool serverChecked: bt.mode === "test"
            onServerCheckedChanged: checked = serverChecked
            Component.onCompleted: checked = serverChecked
            onTriggered: bt.settings.backTapMode = checked ? "test" : "normal"
        }
    }

    Column {
        objectName: "backTapTestPanel"
        visible: bt.mode === "test"
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        spacing: units.gu(1)

        Label {
            width: parent.width
            wrapMode: Text.WordWrap
            fontSize: "small"
            text: i18n.tr("Tap the back of the phone. Every tap the sensor sees is listed here; shortcuts do not run while the test mode is on. It turns itself off after 5 minutes or when the screen turns off.")
        }
        Label {
            visible: bt.entries.length === 0
            width: parent.width
            color: theme.palette.normal.backgroundTertiaryText
            text: i18n.tr("Waiting for taps…")
        }
        Repeater {
            model: bt.entries.slice(0, 8)
            Row {
                spacing: units.gu(1)
                width: parent.width
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: units.gu(1.25)
                    height: width
                    radius: width / 2
                    color: bt.kindColor(modelData.kind)
                }
                Label {
                    width: parent.width - units.gu(2.25)
                    wrapMode: Text.WordWrap
                    font.weight: modelData.kind === "gesture" ? Font.Medium : Font.Normal
                    text: bt.describe(modelData)
                }
            }
        }
    }

    Label {
        visible: bt.mode !== "test"
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        wrapMode: Text.WordWrap
        fontSize: "small"
        text: i18n.tr("Shows every tap the sensor sees and why it did or did not count; no shortcut runs meanwhile.")
    }
}
