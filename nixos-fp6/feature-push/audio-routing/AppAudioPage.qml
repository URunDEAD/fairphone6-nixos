/*
 * Settings > Sound > App audio (FP6). GPL-3.0-only.
 *
 * Apps playing or recording now and the ones with a remembered choice, each
 * with its own output and input ("Default" follows the system default).
 * Data and switching come from ayatana-indicator-sound's actions
 * app-streams (JSON) and set-app-device ("output|input<TAB>app<TAB>id").
 * Calls are not listed: telephony-service routes them.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems
import QMenuModel 1.0

ItemPage {
    id: appAudioPage
    objectName: "appAudioPage"
    title: i18n.tr("App audio")
    flickable: appFlick

    QDBusActionGroup {
        id: group
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.sound"
        objectPath: "/org/ayatana/indicator/sound"

        property variant appStreams: action("app-streams")
        property variant outputDevices: action("output-devices")
        property variant inputDevices: action("input-devices")
        property variant setAppDevice: action("set-app-device")

        Component.onCompleted: start()
    }

    function parse(state) {
        if (!state)
            return []
        try {
            return JSON.parse(state)
        } catch (e) {
            return []
        }
    }

    property var apps: parse(group.appStreams.state)
    property var outputs: parse(group.outputDevices.state)
    property var inputs: parse(group.inputDevices.state)

    function deviceLabel(list, id) {
        if (id === "default")
            return i18n.tr("Default")
        for (var i = 0; i < list.length; i++)
            if (list[i].id === id)
                return list[i].label
        return i18n.tr("Not connected")
    }

    function appByKey(key) {
        for (var i = 0; i < apps.length; i++)
            if (apps[i].key === key)
                return apps[i]
        return null
    }

    Flickable {
        id: appFlick
        anchors.fill: parent
        contentHeight: appColumn.height
        boundsBehavior: (contentHeight > height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: appColumn
            anchors.left: parent.left
            anchors.right: parent.right

            SettingsListItems.Standard {
                visible: appAudioPage.apps.length === 0
                text: i18n.tr("No app is playing or recording. Apps appear here while they use sound and stay listed once you pick a device for them.")
                layout.title.maximumLineCount: 4
                layout.title.wrapMode: Text.WordWrap
                layout.title.color: theme.palette.normal.backgroundSecondaryText
            }

            Repeater {
                objectName: "appList"
                model: appAudioPage.apps

                delegate: ListItem {
                    id: appItem
                    objectName: "app_" + modelData.key
                    height: appLayout.height + (divider.visible ? divider.height : 0)

                    ListItemLayout {
                        id: appLayout
                        title.text: modelData.name
                        subtitle.text: {
                            var parts = []
                            if (modelData.playing)
                                parts.push(i18n.tr("Playing"))
                            if (modelData.recording)
                                parts.push(i18n.tr("Recording"))
                            parts.push(i18n.tr("Output: %1").arg(appAudioPage.deviceLabel(appAudioPage.outputs, modelData.output)))
                            if (modelData.recording || modelData.input !== "default")
                                parts.push(i18n.tr("Input: %1").arg(appAudioPage.deviceLabel(appAudioPage.inputs, modelData.input)))
                            return parts.join(" · ")
                        }
                        subtitle.maximumLineCount: 2
                        subtitle.wrapMode: Text.WordWrap

                        Icon {
                            SlotsLayout.position: SlotsLayout.Leading
                            width: units.gu(4)
                            height: width
                            name: modelData.icon !== "" ? modelData.icon : "application-x-executable"
                        }
                        ProgressionSlot {}
                    }

                    onClicked: pageStack.addPageToNextColumn(appAudioPage, appDevicePage, { appKey: modelData.key })
                }
            }
        }
    }

    Component {
        id: appDevicePage

        ItemPage {
            id: devicePage
            objectName: "appDevicePage"
            property string appKey
            property var app: appAudioPage.appByKey(appKey)
            title: app ? app.name : ""
            flickable: deviceFlick

            Flickable {
                id: deviceFlick
                anchors.fill: parent
                contentHeight: deviceColumn.height

                Column {
                    id: deviceColumn
                    anchors.left: parent.left
                    anchors.right: parent.right

                    Repeater {
                        model: [
                            { kind: "output", title: i18n.tr("Output:") },
                            { kind: "input", title: i18n.tr("Input:") }
                        ]

                        delegate: Column {
                            id: kindColumn
                            anchors.left: parent.left
                            anchors.right: parent.right
                            property string kind: modelData.kind
                            property string current: devicePage.app ? devicePage.app[kind] : "default"
                            property var choices: [{ id: "default", label: i18n.tr("Default"), icon: "" }]
                                                  .concat(kind === "output" ? appAudioPage.outputs : appAudioPage.inputs)

                            SettingsItemTitle { text: modelData.title }

                            Repeater {
                                objectName: kindColumn.kind + "Choices"
                                model: kindColumn.choices

                                delegate: ListItem {
                                    id: choiceItem
                                    objectName: kindColumn.kind + "Choice" + index
                                    height: choiceLayout.height + (divider.visible ? divider.height : 0)
                                    divider.visible: false
                                    property bool active: modelData.id === kindColumn.current

                                    ListItemLayout {
                                        id: choiceLayout
                                        title.text: modelData.label
                                        subtitle.text: modelData.id === "default" ? i18n.tr("Follows the device chosen on the Sound page") : ""

                                        Icon {
                                            SlotsLayout.position: SlotsLayout.Trailing
                                            width: units.gu(2.5)
                                            height: width
                                            name: "tick"
                                            color: theme.palette.normal.activity
                                            visible: choiceItem.active
                                        }
                                    }

                                    onClicked: {
                                        if (!active)
                                            group.setAppDevice.activate(kindColumn.kind + "\t" + devicePage.appKey + "\t" + modelData.id)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
