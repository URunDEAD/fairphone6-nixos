/*
 * FP6 app permissions: everything one app may do: device & data
 * permissions (tap to change), network access per kind of network, last
 * uses, reset. GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6AppPermissionsPage"
    property var model
    property string app
    readonly property bool system: model.isSystem(app)
    readonly property var net: model.network(app)
    readonly property bool anyNet: net.wifi || net.mobile || net.vpn || net.local
    title: model.name(app)
    flickable: flick

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.height + units.gu(4)
        boundsBehavior: contentHeight > height ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: col
            anchors { left: parent.left; right: parent.right }

            Item {
                width: parent.width
                height: units.gu(10)
                Image {
                    id: big
                    anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                    width: units.gu(6); height: width
                    sourceSize { width: width; height: height }
                    source: page.model.icon(page.app)
                    fillMode: Image.PreserveAspectFit
                }
                Column {
                    anchors { left: big.right; leftMargin: units.gu(2); right: parent.right; rightMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                    Label { width: parent.width; text: page.model.name(page.app); textSize: Label.Large; elide: Text.ElideRight }
                    Label {
                        width: parent.width
                        textSize: Label.Small
                        color: theme.palette.normal.backgroundSecondaryText
                        wrapMode: Text.Wrap
                        text: page.system ? i18n.tr("System component: keeps its access")
                                          : page.model.isConfined(page.app) ? i18n.tr("Runs in a sandbox (AppArmor)")
                                                                           : page.app
                    }
                }
            }

            SettingsItemTitle { text: i18n.tr("Network") }

            SettingsListItems.Standard {
                objectName: "fp6NetAll"
                text: i18n.tr("Allow network access")
                enabled: !page.system
                Switch {
                    objectName: "fp6NetAllSwitch"
                    property bool serverChecked: page.anyNet
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: page.model.setNetwork(page.app, "all", checked)
                }
            }
            Repeater {
                model: page.model.netClasses
                delegate: SettingsListItems.Standard {
                    objectName: "fp6Net_" + modelData.id
                    text: modelData.title
                    visible: page.anyNet
                    enabled: !page.system
                    Switch {
                        property bool serverChecked: page.net[modelData.id]
                        onServerCheckedChanged: checked = serverChecked
                        Component.onCompleted: checked = serverChecked
                        onTriggered: page.model.setNetwork(page.app, modelData.id, checked)
                    }
                }
            }
            ListItems.Caption {
                text: page.anyNet
                      ? i18n.tr("Turning a kind of network off blocks this app's traffic over it at once, also open connections. Name lookups still work.")
                      : i18n.tr("This app has no network access at all.")
            }

            SettingsItemTitle { text: i18n.tr("Device & data access") }

            Repeater {
                model: page.model.perms
                delegate: SettingsListItems.SingleValueProgression {
                    objectName: "fp6AppPerm_" + modelData.id
                    text: modelData.title
                    enabled: !page.system
                    progressionVisible: !page.system
                    readonly property var p: page.model.perm(page.app, modelData.id)
                    value: page.model.decisionLabel(page.model.shownDecision(page.app, modelData.id))
                    layout.subtitle.text: p.last_used ? i18n.tr("Last used %1").arg(page.model.timeText(p.last_used).toLowerCase())
                                                        + (p.detail ? " · " + p.detail : "") : ""
                    onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("Fp6DecisionPage.qml"),
                                                             { model: page.model, app: page.app, perm: modelData.id })
                }
            }

            SettingsListItems.StandardProgression {
                objectName: "fp6AppHistory"
                text: i18n.tr("History")
                onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("Fp6PermissionHistoryPage.qml"), { model: page.model, app: page.app })
            }

            SettingsListItems.SingleControl {
                visible: !page.system
                Button {
                    objectName: "fp6AppReset"
                    width: parent.width - units.gu(4)
                    text: i18n.tr("Reset permissions")
                    onClicked: page.model.reset(page.app)
                }
                showDivider: false
            }
            ListItems.Caption {
                visible: !page.system
                text: i18n.tr("Reset: the app asks again next time and gets every network back.")
            }
        }
    }
}
