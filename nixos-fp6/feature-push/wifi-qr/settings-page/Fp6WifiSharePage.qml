// Settings > Wi-Fi > network details > Share (QR code): the network as a
// standard WIFI: QR code (Android/iOS camera apps join from it), its name,
// security and password (hidden until tapped). Reached only after
// WifiShare's check (or with no screen lock / an open network); the
// password and payload are forgotten when the page closes.
import QtQuick 2.12
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Fp6.WifiQr 1.0

ItemPage {
    id: root
    objectName: "fp6WifiSharePage"
    property var share
    property bool showPassword: false

    title: i18n.tr("Share Wi-Fi")
    flickable: flick

    Component.onCompleted: if (!share.revealed) share.reveal()
    Component.onDestruction: share.clear()

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: column
            anchors { left: parent.left; right: parent.right }

            Item { width: 1; height: units.gu(3) }

            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(3) }
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                textSize: Label.Medium
                text: root.share.security === "open" || root.share.security === "owe"
                      ? i18n.tr("Scan this QR code with another device to join “%1”.").arg(root.share.ssid)
                      : i18n.tr("Scan this QR code with another device to join “%1” without typing the password.").arg(root.share.ssid)
            }

            Item { width: 1; height: units.gu(3) }

            // White card: QR readers need dark-on-light, in both themes.
            Rectangle {
                id: card
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(root.width - units.gu(8), units.gu(36))
                height: width
                radius: units.gu(1.5)
                color: "white"
                border.color: theme.palette.normal.base
                border.width: theme.name === "Lomiri.Components.Themes.SuruDark" ? 0 : units.dp(1)

                QrCode {
                    id: qr
                    objectName: "fp6WifiQrCode"
                    anchors.fill: parent
                    anchors.margins: units.gu(1)
                    text: root.share.payload
                    visible: root.share.revealed && text.length > 0
                }
                ActivityIndicator {
                    anchors.centerIn: parent
                    running: !root.share.revealed && root.share.error.length === 0
                }
                Label {
                    anchors { fill: parent; margins: units.gu(3) }
                    visible: root.share.error.length > 0
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    wrapMode: Text.WordWrap
                    color: theme.palette.normal.negative
                    text: root.share.error
                }
            }

            Item { width: 1; height: units.gu(3) }

            SettingsListItems.SingleValue {
                objectName: "fp6ShareName"
                text: i18n.tr("Network name")
                value: root.share.ssid
            }

            SettingsListItems.SingleValue {
                text: i18n.tr("Security")
                value: root.share.securityText
            }

            SettingsListItems.Standard {
                objectName: "fp6SharePassword"
                text: i18n.tr("Password")
                visible: root.share.hasPassword
                onClicked: root.showPassword = !root.showPassword
                Label {
                    objectName: "fp6SharePasswordValue"
                    SlotsLayout.position: SlotsLayout.Trailing
                    width: Math.min(implicitWidth, root.width * 0.55)
                    elide: Text.ElideMiddle
                    text: root.showPassword ? root.share.password : i18n.tr("Tap to show")
                    color: root.showPassword ? theme.palette.normal.backgroundText : theme.palette.normal.backgroundSecondaryText
                }
            }

            SettingsListItems.SingleValue {
                visible: root.share.hidden
                text: i18n.tr("Hidden network")
                value: i18n.tr("Yes")
            }

            Item { width: 1; height: units.gu(2) }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                textSize: Label.Small
                color: theme.palette.normal.backgroundSecondaryText
                text: root.share.hasPassword
                      ? i18n.tr("Anyone who sees this code can join the network. The password stays hidden here until you tap it.")
                      : i18n.tr("This network has no password.")
            }
        }
    }
}
