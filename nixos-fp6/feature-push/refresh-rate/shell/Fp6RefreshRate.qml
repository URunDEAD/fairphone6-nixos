/*
 * FP6 refresh rate ("Smooth display") in the shell. Instantiated once per
 * ShellScreen (only the primary one is active). Applies
 * com.fairphone6.refresh-rate through two paths:
 *  * DRM mode (fixed modes 30/60/90/120 Hz): WindowManager.Screen
 *    beginConfiguration()/applyConfiguration() -> qtmir ScreensController
 *    -> Mir set_base_configuration -> KMS modeset. The panel is
 *    re-initialised, so the screen blanks for a moment.
 *  * Panel runtime rate (adaptive; kernel candidate 0010 panel_rate_hz):
 *    no modeset, the mode stays 120 Hz and the panel skips frames when idle.
 * Publishes available-rates, adaptive-available and active-rate for
 * System Settings and the quick-settings tile.
 * Policy: Fp6RefreshRate.js plan().
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.15
import GSettings 1.0
import FP6.RefreshRate 1.0 as FP6RR
import "Fp6RefreshRate.js" as RR

Item {
    id: root

    // WindowManager ConcreteScreen and the QQuickWindow of this ShellScreen
    property var screen: null
    property var window: null
    // only the primary screen's instance acts
    property bool active: true
    // for the laptop test: a fake panel_rate_hz, a short start delay
    property alias panelPath: panel.path
    property int startDelay: 1500

    GSettings {
        id: rr
        objectName: "fp6RefreshSettings"
        schema.id: "com.fairphone6.refresh-rate"
    }
    GSettings {
        id: battery
        objectName: "fp6BatterySettings"
        schema.id: "com.fairphone6.battery"
    }

    FP6RR.PanelRate {
        id: panel
        objectName: "fp6PanelRate"
    }

    FP6RR.ActivityWatcher {
        id: activity
        objectName: "fp6Activity"
        target: root.window
        idleMs: rr.adaptiveIdleMs || 3000
        enabled: root.active && root.mode === "adaptive" && panel.available
    }

    readonly property string mode: rr.mode || "high"
    readonly property bool saver: battery.saverActive === true
    readonly property bool saverCap: rr.saverCap !== false
    property var modes: []
    property int currentIndex: -1
    readonly property var rates: RR.ratesOf(modes, currentIndex)
    readonly property var plan: RR.plan({
        mode: mode, saver: saver, saverCap: saverCap,
        touching: activity.active, idleRate: rr.adaptiveIdleRate || 60,
        rates: rates, runtime: panel.available
    })
    property bool started: false

    function screenOn() {
        // MirPowerMode: 0 = on. Unknown (not exported) counts as on.
        return !screen || screen.powerMode === undefined || screen.powerMode === 0;
    }

    function readModes() {
        modes = RR.modesOf(screen);
        currentIndex = screen ? screen.currentModeIndex : -1;
        publish();
    }

    function publish() {
        if (!active)
            return;
        var s = rates.join(",");
        if (rr.availableRates !== undefined && rr.availableRates !== s)
            rr.availableRates = s;
        if (rr.adaptiveAvailable !== undefined && rr.adaptiveAvailable !== panel.available)
            rr.adaptiveAvailable = panel.available;
        var cur = (currentIndex >= 0 && currentIndex < modes.length) ? Math.round(modes[currentIndex].hz) : 0;
        var act = (panel.available && panel.current > 0 && panel.current < cur) ? panel.current : cur;
        if (rr.activeRate !== undefined && rr.activeRate !== act)
            rr.activeRate = act;
    }

    function applyMode() {
        if (!started || !active || !screen || plan.modeHz <= 0)
            return;
        var idx = RR.pickMode(modes, currentIndex, plan.modeHz);
        if (idx < 0 || idx === currentIndex)
            return;
        var cfg = screen.beginConfiguration();
        if (!cfg)
            return;
        cfg.currentModeIndex = idx;
        if (screen.applyConfiguration(cfg))
            console.info("fp6 refresh rate: mode", plan.modeHz, "Hz (" + mode + ")");
        else
            console.warn("fp6 refresh rate: Mir refused mode", idx, "(" + plan.modeHz + " Hz)");
    }

    function applyPanel() {
        if (!started || !active || !panel.available || plan.panelHz <= 0 || !screenOn())
            return;
        if (panel.current !== plan.panelHz)
            panel.setRate(plan.panelHz);
    }

    function applyAll() {
        applyMode();
        applyPanel();
    }

    // Deferred: a modeset changes the screen's modes, which feed plan.
    onPlanChanged: Qt.callLater(root.applyAll)
    onScreenChanged: readModes()
    onActiveChanged: publish()

    Connections {
        target: root.screen
        ignoreUnknownSignals: true
        function onAvailableModesChanged() { root.readModes(); }
        function onCurrentModeIndexChanged() {
            // a modeset resets the panel's runtime rate to the mode's rate
            root.readModes();
            panel.refresh();
            Qt.callLater(root.applyPanel);
        }
        function onPowerModeChanged() {
            panel.refresh();
            Qt.callLater(root.applyPanel);
        }
    }
    Connections {
        target: panel
        function onCurrentChanged() { root.publish(); }
        function onAvailableChanged() { root.publish(); }
        function onLastErrorChanged() {
            if (panel.lastError !== "")
                console.warn("fp6 refresh rate: panel_rate_hz:", panel.lastError);
        }
    }

    // Start after Mir's initial configuration has settled.
    Timer {
        interval: root.startDelay
        running: true
        onTriggered: {
            // The shell (re)started during a Settings trial of a new mode:
            // nobody confirmed it, so go back to the default.
            if (root.active && rr.confirmed === false) {
                console.warn("fp6 refresh rate: unconfirmed mode", rr.mode, "-> high");
                rr.mode = "high";
                rr.confirmed = true;
            }
            root.readModes();
            panel.refresh();
            root.started = true;
            root.applyAll();
        }
    }
}
