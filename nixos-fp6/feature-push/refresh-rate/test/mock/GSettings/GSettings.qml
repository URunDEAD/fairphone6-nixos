// Mock of gsettings-qt's GSettings for the laptop test: every key of the
// schemas the refresh-rate code uses, as plain writable properties with the
// schema defaults (com.fairphone6.refresh-rate, com.fairphone6.battery).
import QtQuick 2.15
import GSettingsMock 1.0

QtObject {
    property MockSchema schema: MockSchema {}
    // com.fairphone6.refresh-rate
    property var mode: "high"
    property var adaptiveIdleRate: 60
    property var adaptiveIdleMs: 3000
    property var saverCap: true
    property var smoothMode: "high"
    property var confirmed: true
    property var confirmedModes: "high"
    property var availableRates: ""
    property var adaptiveAvailable: false
    property var activeRate: 0
    // com.fairphone6.battery
    property var saverActive: false
}
