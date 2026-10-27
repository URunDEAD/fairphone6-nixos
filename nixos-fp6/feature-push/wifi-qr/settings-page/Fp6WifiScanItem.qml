// Settings > Wi-Fi: "Scan QR code to connect" opens the Wi-Fi QR scanner
// (fp6-wifi-qr-scan, its own app so the camera in-use indicator names it).
import QtQuick 2.12
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Fp6.WifiQr 1.0

SettingsListItems.IconProgression {
    objectName: "fp6ScanQr"
    text: i18n.tr("Scan QR code to connect")
    iconSource: Qt.resolvedUrl("fp6-icons/fp6-qrcode-symbolic.svg")
    visible: WifiQr.scannerAvailable
    onClicked: WifiQr.launchScanner()
}
