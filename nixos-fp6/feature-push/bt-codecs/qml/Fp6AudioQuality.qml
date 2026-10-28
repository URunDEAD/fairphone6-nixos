import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems

Column {
    id: root
    property var audio
    property string address: ""
    property bool connected: false
    property var device: {
        var list = audio ? audio.devices : []
        for (var i = 0; i < list.length; ++i)
            if (list[i].address.toUpperCase() === address.toUpperCase()) return list[i]
        return null
    }
    property var profiles: device ? device.profiles.filter(function(p) { return p.available }) : []
    property bool callProfile: device ? device.profile.indexOf("headset-head-unit") === 0 : false
    spacing: units.gu(1)
    visible: !!audio && audio.available

    Label {
        text: i18n.tr("Audio quality")
        font.bold: true
        anchors.left: parent.left
        anchors.leftMargin: units.gu(2)
    }
    ListItems.Caption {
        visible: !root.device
        text: root.connected ? i18n.tr("Waiting for an audio connection…") : i18n.tr("Connect this device to choose audio quality.")
    }
    ListItems.SingleValue {
        visible: !!root.device
        text: i18n.tr("Current codec")
        value: root.device ? root.device.codec.toUpperCase().replace(/_/g, "-") : ""
    }
    ListItems.ItemSelector {
        objectName: "audioCodecPicker"
        visible: !!root.device
        enabled: !!root.device && !root.audio.busy
        text: i18n.tr("Codec and profile")
        expanded: false
        model: root.profiles.map(function(p) { return p.kind + " · " + p.codec.toUpperCase().replace(/_/g, "-") })
        selectedIndex: {
            for (var i = 0; i < root.profiles.length; ++i)
                if (root.device && root.profiles[i].id === root.device.profile) return i
            return -1
        }
        onDelegateClicked: root.audio.selectProfile(root.device.id, root.profiles[index].id)
    }
    ListItems.Standard {
        visible: !!root.device
        text: i18n.tr("Prefer quality")
        control: Switch {
            objectName: "preferAudioQuality"
            checked: root.device ? root.device.codec !== "sbc" : false
            enabled: !!root.device && !root.audio.busy && !root.callProfile
            onTriggered: root.audio.prefer(root.device.id, checked)
        }
    }
    ListItems.Caption {
        visible: !!root.device
        text: root.callProfile ? i18n.tr("Call profile active. Music quality is reduced while the microphone is used.")
              : i18n.tr("Turn off to prefer a stable connection using SBC. Your selection is remembered for this device.")
    }
    ListItems.SingleValue {
        visible: !!root.device
        text: i18n.tr("Bitrate")
        value: root.device && root.device.bitrate ? Math.round(root.device.bitrate / 1000) + " kbit/s" : i18n.tr("Not reported")
    }
    ListItems.Caption {
        visible: !!root.device && !!root.device.bitrate
        text: root.device ? (root.device.bitrate_source || "") : ""
    }
    ListItems.Caption {
        objectName: "audioError"
        visible: root.audio && root.audio.error.length > 0
        text: root.audio ? root.audio.error : ""
    }
    ListItems.Caption {
        visible: root.audio && root.audio.busy
        text: i18n.tr("Updating audio…")
    }
}
