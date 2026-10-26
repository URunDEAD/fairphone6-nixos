// SPDX-License-Identifier: LGPL-3.0-only
import QtQuick 2.4
import Biometryd 0.0
import Lomiri.Components 1.3
import Lomiri.Settings.Fingerprint 0.1

MainView {
    id: root
    applicationName: "fp6-fingerprint-id"
    width: units.gu(45)
    height: units.gu(80)

    // FingerprintsView (lomiri-settings-components) resolves `pageStack` from
    // its creation context, exactly as inside System Settings.
    PageStack {
        id: pageStack
        Component.onCompleted: push(Biometryd.available ? fingerprintsPage : unavailablePage)
    }

    Component {
        id: fingerprintsPage
        Fingerprints {
            // Fingerprint unlock only makes sense with a passcode/password lock;
            // the Settings toggle enforces that. Enrolment itself does not need it.
            passcodeSet: true
        }
    }

    Component {
        id: unavailablePage
        Page {
            header: PageHeader { title: "Fingerprint ID" }
            Label {
                anchors { fill: parent; margins: units.gu(3) }
                wrapMode: Text.WordWrap
                text: "The fingerprint service (biometryd) is not running.\n\n" +
                      "Start the fingerprint daemon, then open this app again."
            }
        }
    }
}
