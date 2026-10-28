// Fp6RefreshRate.js policy (laptop test, qmltestrunner -platform offscreen)
import QtQuick 2.15
import QtTest 1.15
import "Fp6RefreshRate.js" as RR

TestCase {
    name: "RefreshRatePolicy"

    function p(o) {
        var d = { mode: "high", saver: false, saverCap: true, touching: false,
                  idleRate: 60, rates: [30, 60, 90, 120], runtime: false };
        for (var k in o) d[k] = o[k];
        return RR.plan(d);
    }

    function test_parse() {
        compare(RR.parseRates("120,60,90"), [60, 90, 120]);
        compare(RR.parseRates("120 60 30 10"), [10, 30, 60, 120]);
        compare(RR.parseRates(""), []);
        compare(RR.parseRates(undefined), []);
    }
    function test_fixed() {
        compare(p({ mode: "high" }).modeHz, 120);
        compare(p({ mode: "medium" }).modeHz, 90);
        compare(p({ mode: "standard" }).modeHz, 60);
        compare(p({ mode: "low" }).modeHz, 30);
        compare(p({ mode: "nonsense" }).modeHz, 120);
        compare(p({ mode: "standard" }).panelHz, 0);
    }
    function test_kernel6_single_mode() {
        // only 120 Hz: every mode maps to it, nothing to switch
        compare(p({ mode: "standard", rates: [120] }).modeHz, 120);
        compare(p({ mode: "adaptive", rates: [120] }).modeHz, 120);
        compare(p({ mode: "adaptive", rates: [120] }).panelHz, 0);
        compare(p({ mode: "high", rates: [] }).modeHz, 0);
    }
    function test_missing_rate_falls_back_lower() {
        compare(p({ mode: "medium", rates: [60, 120] }).modeHz, 60);
        compare(p({ mode: "low", rates: [60, 120] }).modeHz, 60);
    }
    function test_saver_cap() {
        compare(p({ mode: "high", saver: true }).modeHz, 60);
        compare(p({ mode: "medium", saver: true }).modeHz, 60);
        compare(p({ mode: "low", saver: true }).modeHz, 30);
        compare(p({ mode: "high", saver: true, saverCap: false }).modeHz, 120);
    }
    function test_adaptive() {
        var a = p({ mode: "adaptive", runtime: true, touching: true });
        compare(a.modeHz, 120); compare(a.panelHz, 120);
        a = p({ mode: "adaptive", runtime: true, touching: false });
        compare(a.modeHz, 120); compare(a.panelHz, 60);
        a = p({ mode: "adaptive", runtime: true, idleRate: 10 });
        compare(a.panelHz, 10);
        a = p({ mode: "adaptive", runtime: true, idleRate: 30 });
        compare(a.panelHz, 30);
        // battery saver: no modeset, the panel rate is capped
        a = p({ mode: "adaptive", runtime: true, touching: true, saver: true });
        compare(a.modeHz, 120); compare(a.panelHz, 60);
        a = p({ mode: "adaptive", runtime: true, touching: false, saver: true, idleRate: 10 });
        compare(a.panelHz, 10);
        // no kernel support: like high (capped)
        a = p({ mode: "adaptive", runtime: false });
        compare(a.modeHz, 120); compare(a.panelHz, 0);
        compare(p({ mode: "adaptive", runtime: false, saver: true }).modeHz, 60);
    }
    function test_pick_mode() {
        var m = [{ width: 1116, height: 2484, hz: 119.9999 }, { width: 1116, height: 2484, hz: 89.9998 },
                 { width: 1116, height: 2484, hz: 59.9998 }, { width: 800, height: 600, hz: 60 }];
        compare(RR.pickMode(m, 0, 60), 2);
        compare(RR.pickMode(m, 0, 120), 0);
        compare(RR.pickMode(m, 0, 30), -1);
        compare(RR.pickMode(m, 9, 60), -1);
        compare(RR.ratesOf(m, 0), [60, 90, 120]);
    }
    function test_lists() {
        verify(RR.listHas("high,standard", "standard"));
        verify(!RR.listHas("high", "low"));
        compare(RR.listAdd("high", "low"), "high,low");
        compare(RR.listAdd("high", "high"), "high");
        compare(RR.listAdd("", "low"), "low");
    }
    function test_effective() {
        compare(RR.effectiveRate("high", true, true, [60, 120], false), 60);
        compare(RR.effectiveRate("adaptive", false, true, [120], true), 120);
    }
}
