// Mock of Lomiri's WindowManager.Screen (ConcreteScreen) for the test.
import QtQuick 2.15

QtObject {
    id: screen
    property var rates: [120]
    property var availableModes: rates.map(function (r) {
        return { size: { width: 1116, height: 2484 }, refreshRate: r * 0.99999 };
    })
    property int currentModeIndex: 0
    property int powerMode: 0
    property int applied: 0
    function beginConfiguration() { return { currentModeIndex: screen.currentModeIndex }; }
    function applyConfiguration(cfg) {
        applied++;
        currentModeIndex = cfg.currentModeIndex;
        return true;
    }
    function rate() { return rates[currentModeIndex]; }
}
