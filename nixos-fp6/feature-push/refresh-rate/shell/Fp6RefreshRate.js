/*
 * FP6 refresh rate ("Smooth display"): the policy, as pure functions shared
 * by the shell component (Fp6RefreshRate.qml), System Settings and the
 * laptop test (test/tst_policy.qml).
 * SPDX-License-Identifier: GPL-3.0-only
 */
.pragma library

// Fixed modes and their rate. "adaptive" is handled in plan().
var fixedRates = { "low": 30, "standard": 60, "medium": 90, "high": 120 };
var maxRate = 120;
var saverRate = 60;

// "60,90,120" -> [60, 90, 120]
function parseRates(s) {
    var out = [];
    var parts = String(s || "").split(/[ ,]+/);
    for (var i = 0; i < parts.length; i++) {
        var n = parseInt(parts[i], 10);
        if (n > 0 && out.indexOf(n) < 0)
            out.push(n);
    }
    out.sort(function (a, b) { return a - b; });
    return out;
}

// The highest available rate not above want; the lowest one if all are
// above; 0 if nothing is known.
function bestAvailable(want, rates) {
    if (!rates || rates.length === 0)
        return 0;
    var best = 0;
    for (var i = 0; i < rates.length; i++)
        if (rates[i] <= want && rates[i] > best)
            best = rates[i];
    return best > 0 ? best : rates[0];
}

// The runtime panel rates (kernel panel_rate_hz) are 120/60/30/10 on the
// 120 Hz scan; pick the highest one not above want and not above modeHz.
var runtimeRates = [10, 30, 60, 120];
function bestRuntime(want, modeHz) {
    var best = runtimeRates[0];
    for (var i = 0; i < runtimeRates.length; i++)
        if (runtimeRates[i] <= want && runtimeRates[i] <= modeHz)
            best = runtimeRates[i];
    return best;
}

/*
 * p: { mode, saver (battery saver active), saverCap (setting), touching,
 *      idleRate (adaptive, Hz), rates ([Hz] of the DRM modes),
 *      runtime (the panel has panel_rate_hz) }
 * Returns { modeHz, panelHz }: the DRM mode to use (0 = leave it) and the
 * runtime panel rate to write (0 = do not write).
 *  * Fixed modes are real modes (Mir modeset; the screen blanks briefly).
 *  * Adaptive keeps the highest mode and moves only the panel's own rate:
 *    the mode rate while touched, idleRate when idle. No modeset, so the
 *    switches are invisible. Without kernel support it acts as "high".
 *  * Battery saver (with saverCap) caps everything at 60 Hz; in adaptive
 *    mode through the panel rate only (no blank).
 */
function plan(p) {
    var rates = p.rates || [];
    var cap = (p.saver && p.saverCap) ? saverRate : maxRate;
    if (p.mode === "adaptive") {
        var modeHz = bestAvailable(maxRate, rates);
        if (!p.runtime || modeHz === 0)
            return { modeHz: bestAvailable(cap, rates), panelHz: 0 };
        var high = bestRuntime(Math.min(cap, modeHz), modeHz);
        var low = bestRuntime(Math.min(p.idleRate || saverRate, high), modeHz);
        return { modeHz: modeHz, panelHz: p.touching ? high : low };
    }
    var want = fixedRates.hasOwnProperty(p.mode) ? fixedRates[p.mode] : maxRate;
    return { modeHz: bestAvailable(Math.min(want, cap), rates), panelHz: 0 };
}

// The rate a mode shows (for labels): plan() while touched.
function effectiveRate(mode, saver, saverCap, rates, runtime) {
    var r = plan({ mode: mode, saver: saver, saverCap: saverCap, touching: true,
                   idleRate: saverRate, rates: rates, runtime: runtime });
    return r.panelHz > 0 ? r.panelHz : r.modeHz;
}

// modes: [{ width, height, hz }]. The index of the mode with the current
// mode's size and the wanted rate (+-0.5 Hz), preferring the current one;
// -1 if there is none.
function pickMode(modes, currentIndex, hz) {
    if (!modes || currentIndex < 0 || currentIndex >= modes.length)
        return -1;
    var cur = modes[currentIndex];
    var best = -1;
    for (var i = 0; i < modes.length; i++) {
        var m = modes[i];
        if (m.width !== cur.width || m.height !== cur.height)
            continue;
        if (Math.abs(m.hz - hz) >= 0.5)
            continue;
        if (best < 0 || i === currentIndex)
            best = i;
    }
    return best;
}

// Rounded rates offered at the current mode's size, ascending.
function ratesOf(modes, currentIndex) {
    var rates = [];
    if (!modes || currentIndex < 0 || currentIndex >= modes.length)
        return rates;
    var cur = modes[currentIndex];
    for (var i = 0; i < modes.length; i++) {
        var m = modes[i];
        if (m.width !== cur.width || m.height !== cur.height)
            continue;
        var r = Math.round(m.hz);
        if (rates.indexOf(r) < 0)
            rates.push(r);
    }
    rates.sort(function (a, b) { return a - b; });
    return rates;
}

// Plain copies of WindowManager.Screen.availableModes (qtmir::ScreenMode).
function modesOf(screen) {
    var out = [];
    if (!screen || !screen.availableModes)
        return out;
    var list = screen.availableModes;
    for (var i = 0; i < list.length; i++) {
        var m = list[i];
        out.push({ width: m.size.width, height: m.size.height, hz: m.refreshRate });
    }
    return out;
}

// "high,standard" contains mode?
function listHas(list, item) {
    return String(list || "").split(",").indexOf(item) >= 0;
}
function listAdd(list, item) {
    if (listHas(list, item))
        return String(list);
    return list ? String(list) + "," + item : item;
}
