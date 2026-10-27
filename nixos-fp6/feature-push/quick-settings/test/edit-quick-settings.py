import sys
root = sys.argv[1]
def sub(path, old, new, count=1):
    p = f"{root}/{path}"
    s = open(p).read()
    n = s.count(old)
    if n != count:
        sys.exit(f"{path}: expected {count} of {old!r}, found {n}")
    open(p, "w").write(s.replace(old, new))

# ---------------- PanelItemRow: hide highlight, look up items by identifier
sub("qml/Panel/PanelItemRow.qml",
"""    property real lateralPosition: -1
    onLateralPositionChanged: {""",
"""    property real lateralPosition: -1
    // FP6 quick settings: no tab highlight while the control centre is shown
    property bool fp6HideHighlight: false
    function fp6ItemFor(identifier) {
        for (var i = 0; i < row.contentItem.children.length; i++) {
            var c = row.contentItem.children[i];
            if (c.hasOwnProperty("ownIndex") && c.identifier === identifier) return c;
        }
        return null;
    }
    onLateralPositionChanged: {""")
sub("qml/Panel/PanelItemRow.qml",
"""        visible: currentItem !== null
        opacity: 0.0""",
"""        visible: currentItem !== null && !root.fp6HideHighlight
        opacity: 0.0""")

# ---------------- PanelBar: tab taps, item lookup
sub("qml/Panel/PanelBar.qml",
"""    property alias hideRow: row.hideRow""",
"""    property alias hideRow: row.hideRow
    // FP6 quick settings
    property alias fp6HideHighlight: row.fp6HideHighlight
    readonly property alias fp6CurrentItem: row.currentItem
    signal fp6TabClicked()
    function fp6ItemFor(identifier) { return row.fp6ItemFor(identifier); }""")
sub("qml/Panel/PanelBar.qml",
"""                    onClicked: {
                        row.selectItemAt(mouse.x);
                        d.alignIndicators();
                    }""",
"""                    onClicked: {
                        row.selectItemAt(mouse.x);
                        d.alignIndicators();
                        root.fp6TabClicked();
                    }""")

# ---------------- PanelMenu: control centre overlay
sub("qml/Panel/PanelMenu.qml",
"""    property real fp6SafeLeft: 0
    property real fp6SafeRight: 0
""",
"""    property real fp6SafeLeft: 0
    property real fp6SafeRight: 0

    // FP6 quick settings: a page (the control centre) drawn over the menu
    // pages. It opens first when fp6OverlayDefault is set; tapping an item in
    // the expanded row shows that indicator's page, and the "Quick settings"
    // strip above the page goes back.
    property Component fp6Overlay: null
    property bool fp6OverlayDefault: true
    property bool fp6OverlayShown: fp6OverlayDefault
    // set once the menu was opened (or by the panel after start-up): the overlay
    // is created then and kept; it is invisible (not rendered) while closed.
    property bool fp6EverOpened: false
    readonly property alias fp6OverlayItem: fp6OverlayLoader.item
    property bool fp6Locked: false
    property var fp6LockedRestricted: []
    readonly property string fp6CurrentIdentifier: bar.fp6CurrentItem && bar.fp6CurrentItem.identifier !== undefined
                                                   ? bar.fp6CurrentItem.identifier : ""
    signal fp6UnlockRequested()
    onStateChanged: if (state === "initial") fp6OverlayShown = Qt.binding(function() { return fp6OverlayDefault && fp6Overlay !== null; })
    function fp6OpenIndicator(identifier) {
        var item = bar.fp6ItemFor(identifier);
        if (!item) return false;
        bar.setCurrentItemIndex(item.ownIndex);
        fp6OverlayShown = false;
        return true;
    }
    function fp6FindIndicator(identifier) {
        var item = bar.fp6ItemFor(identifier);
        return item ? { busName: item.busName, actionsObjectPath: item.actionsObjectPath,
                        menuObjectPath: item.menuObjectPath } : null;
    }
""")
sub("qml/Panel/PanelMenu.qml",
"""        MenuContent {
            id: content
            objectName: "menuContent"

            anchors {
                left: parent.left
                right: parent.right
                top: parent.top
            }
            height: openedHeight - bar.height - handle.height
            model: root.model
            visible: root.unitProgress > 0
            currentMenuIndex: bar.currentItemIndex
        }
    }""",
"""        // FP6 quick settings: back from an indicator page to the control centre
        AbstractButton {
            id: fp6BackStrip
            objectName: "fp6QuickSettingsBack"
            visible: root.fp6Overlay !== null && !root.fp6OverlayShown
            anchors { left: parent.left; right: parent.right; top: parent.top }
            height: visible ? units.gu(5) : 0
            onClicked: root.fp6OverlayShown = true
            Row {
                anchors { left: parent.left; leftMargin: units.gu(2) + root.fp6SafeLeft; verticalCenter: parent.verticalCenter }
                spacing: units.gu(1)
                Icon {
                    name: "go-previous"
                    width: units.gu(2); height: width
                    anchors.verticalCenter: parent.verticalCenter
                    color: theme.palette.normal.backgroundText
                }
                Icon {
                    name: "view-grid-symbolic"
                    width: units.gu(2); height: width
                    anchors.verticalCenter: parent.verticalCenter
                    color: theme.palette.normal.backgroundText
                }
                Label {
                    text: i18n.tr("Quick settings")
                    anchors.verticalCenter: parent.verticalCenter
                    color: theme.palette.normal.backgroundText
                }
            }
            Rectangle {
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: units.dp(1)
                color: theme.palette.normal.base
                opacity: 0.5
            }
        }

        MenuContent {
            id: content
            objectName: "menuContent"

            anchors {
                left: parent.left
                right: parent.right
                top: fp6BackStrip.bottom
            }
            height: openedHeight - bar.height - handle.height - fp6BackStrip.height
            model: root.model
            // FP6: not drawn under the control centre
            visible: root.unitProgress > 0 && !(root.fp6OverlayShown && fp6OverlayLoader.status == Loader.Ready)
            currentMenuIndex: bar.currentItemIndex
        }

        // FP6 privacy: on the lock screen some indicator pages (Wi-Fi networks,
        // Bluetooth devices, location, ...) need the passcode first.
        Rectangle {
            objectName: "fp6LockedPage"
            anchors.fill: content
            visible: content.visible && root.fp6Locked
                     && root.fp6LockedRestricted.indexOf(root.fp6CurrentIdentifier) >= 0
            color: Qt.rgba(root.panelColor.r, root.panelColor.g, root.panelColor.b, 1.0)
            MouseArea { anchors.fill: parent }
            Column {
                anchors.centerIn: parent
                width: parent.width - units.gu(8)
                spacing: units.gu(2)
                Icon {
                    name: "system-lock-screen"
                    width: units.gu(4); height: width
                    anchors.horizontalCenter: parent.horizontalCenter
                    color: theme.palette.normal.backgroundText
                }
                Label {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: i18n.tr("Unlock the phone to change these settings.")
                    color: theme.palette.normal.backgroundText
                }
                Button {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Unlock")
                    onClicked: root.fp6UnlockRequested()
                }
            }
        }

        Loader {
            id: fp6OverlayLoader
            objectName: "fp6OverlayLoader"
            anchors {
                left: parent.left
                right: parent.right
                top: parent.top
            }
            height: openedHeight - bar.height - handle.height
            active: root.fp6Overlay !== null && root.fp6EverOpened
            sourceComponent: root.fp6Overlay
            visible: root.fp6OverlayShown && root.unitProgress > 0 && status == Loader.Ready
        }
    }""")
sub("qml/Panel/PanelMenu.qml",
"""        lightMode: root.lightMode
        unitProgress: root.unitProgress

        height: expanded ? expandedPanelHeight : minimizedPanelHeight""",
"""        lightMode: root.lightMode
        unitProgress: root.unitProgress
        fp6HideHighlight: root.fp6OverlayShown && root.fp6Overlay !== null
        onFp6TabClicked: root.fp6OverlayShown = false

        height: expanded ? expandedPanelHeight : minimizedPanelHeight""")
sub("qml/Panel/PanelMenu.qml",
"""                enableLateralChanges: d.activeDragHandle &&
                                      !yVelocityCalculator.velocityAboveThreshold""",
"""                // FP6: no sideways indicator switching while the control centre opens
                enableLateralChanges: d.activeDragHandle &&
                                      !yVelocityCalculator.velocityAboveThreshold &&
                                      !(root.fp6OverlayShown && root.fp6Overlay !== null)""")

sub("qml/Panel/PanelMenu.qml",
"""    onUnitProgressChanged: d.updateState()
""",
"""    onUnitProgressChanged: {
        d.updateState();
        if (unitProgress > 0 && !fp6EverOpened) fp6EverOpened = true; // FP6 quick settings
    }
""")

# ---------------- Panel.qml
sub("qml/Panel/Panel.qml",
"""import "Indicators"
""",
"""import "Indicators"
import "QuickSettings"
""")
sub("qml/Panel/Panel.qml",
"""    property bool screenRecording: false
    signal screenRecordingStopClicked()
""",
"""    property bool screenRecording: false
    signal screenRecordingStopClicked()

    // FP6 quick settings (feature-push/quick-settings): the control centre in
    // the indicators menu. Set by Shell.qml.
    property bool fp6Locked: false
    property var fp6Notifications: null
    property bool fp6HideNotificationContent: false
    signal fp6UnlockRequested()
    signal fp6ScreenshotRequested()
    signal fp6AppRequested(string appId)
    // indicator pages that need the passcode on the lock screen (module option)
    readonly property var fp6LockedRestricted: @fp6LockedRestricted@
    readonly property alias fp6QuickSettings: fp6QsLoader.item

    // The tile backends (indicator action groups, GSettings) are created when
    // the menu is first opened or 15 s after start, whichever comes first.
    Timer {
        id: fp6QsWarmup
        interval: 15000
        running: true
        onTriggered: __indicators.fp6EverOpened = true
    }
    Loader {
        id: fp6QsLoader
        active: __indicators.fp6EverOpened
        sourceComponent: QsBackend {
            findIndicator: function(identifier) { return __indicators.fp6FindIndicator(identifier); }
            shown: __indicators.fp6OverlayShown && !__indicators.fullyClosed
            onScreenshotRequested: root.fp6ScreenshotRequested()
        }
    }
    Component {
        id: fp6ControlCentre
        ControlCentre {
            backend: fp6QsLoader.item
            lightMode: root.lightMode
            locked: root.fp6Locked
            notificationsModel: root.fp6Notifications
            hideNotificationContent: root.fp6HideNotificationContent
            lockedRestricted: root.fp6LockedRestricted
            bottomInset: root.fp6CornerInset(units.gu(2)) > 0 ? units.gu(2) : 0
            indicatorExists: function(identifier) { return __indicators.fp6FindIndicator(identifier) !== null; }
            onOpenIndicator: __indicators.fp6OpenIndicator(identifier)
            onOpenUrl: { __indicators.hide(); Qt.openUrlExternally(url); }
            onUnlockRequested: root.fp6UnlockRequested()
            onOpenApp: root.fp6AppRequested(appId)
            onVisibleChanged: if (visible && backend) backend.indicatorsRevision++
        }
    }
    Loader {
        anchors.fill: parent
        z: 1000
        active: !!fp6QsLoader.item && !!fp6QsLoader.item.settings.showSafeArea
        sourceComponent: QsSafeArea {
            radius: root.fp6CornerRadius
            barHeight: root.minimizedPanelHeight
            inset: root.fp6SafeInset
        }
    }
""")
sub("qml/Panel/Panel.qml",
"""            enableHint: !callHint.active && !fullscreenMode
            showOnClick: !callHint.visible
            panelColor: panelAreaBackground.color""",
"""            enableHint: !callHint.active && !fullscreenMode
            showOnClick: !callHint.visible
            panelColor: panelAreaBackground.color
            // FP6 quick settings
            fp6Overlay: fp6QsLoader.item ? fp6ControlCentre : null
            fp6OverlayDefault: !fp6QsLoader.item || fp6QsLoader.item.openControlCentre
            fp6Locked: root.fp6Locked
            fp6LockedRestricted: root.fp6LockedRestricted
            onFp6UnlockRequested: root.fp6UnlockRequested()""")
sub("qml/Panel/Panel.qml",
"""                expanded: indicators.expanded
                selected: ListView.isCurrentItem
""",
"""                expanded: indicators.expanded
                selected: ListView.isCurrentItem && !(__indicators.fp6OverlayShown && __indicators.fp6Overlay !== null)
""")

# ---------------- Notifications.qml: record into the panel list
sub("qml/Notifications/Notifications.qml",
"""    readonly property bool useModal: snapDecisionProxyModel.count > 0
""",
"""    readonly property bool useModal: snapDecisionProxyModel.count > 0

    // FP6 quick settings: a copy of each notification for the pulled-down
    // panel, grouped by app (newest group first), kept until dismissed there.
    property var fp6Shade: null
    function fp6Record(type, hints, icon, summary, body) {
        if (!fp6Shade || !summary) return;
        if (type === LomiriNotifications.Notification.SnapDecision ||
            type === LomiriNotifications.Notification.Confirmation ||
            type === LomiriNotifications.Notification.PlaceHolder) return;
        var h = hints || {};
        if (h["x-lomiri-private-synchronous"] || h["x-fp6-no-shade"] === "true") return;
        var app = h["x-fp6-app-name"] || h["desktop-entry"] || i18n.tr("Notifications");
        var appId = String(h["desktop-entry"] || "").replace(/\\.desktop$/, "");
        var entry = { app: String(app), appId: appId, icon: String(icon || ""),
                      summary: String(summary), body: String(body || ""), time: Date.now() };
        var first = -1, last = -1;
        for (var i = 0; i < fp6Shade.count; i++) {
            if (fp6Shade.get(i).app === entry.app) { if (first < 0) first = i; last = i; }
        }
        if (first < 0) {
            fp6Shade.insert(0, entry);
        } else {
            fp6Shade.insert(first, entry);
            if (first > 0) fp6Shade.move(first, 0, last - first + 2);
        }
        while (fp6Shade.count > 60) fp6Shade.remove(fp6Shade.count - 1);
    }
""")
sub("qml/Notifications/Notifications.qml",
"""        Component.onCompleted: topmostIsFullscreen = false; // async, the factory loader will set fullscreen to true later""",
"""        Component.onCompleted: {
            topmostIsFullscreen = false; // async, the factory loader will set fullscreen to true later
            notificationList.fp6Record(model.type, model.hints, model.icon, model.summary, model.body); // FP6
        }""")

# ---------------- Shell.qml
sub("qml/Shell.qml",
"""            screenRecording: screenRecorder.recording
            onScreenRecordingStopClicked: screenRecorder.stop()
""",
"""            screenRecording: screenRecorder.recording
            onScreenRecordingStopClicked: screenRecorder.stop()
            // FP6 quick settings
            fp6Locked: !!greeter && greeter.shown && greeter.locked
            fp6Notifications: fp6NotificationShade
            fp6HideNotificationContent: notifications.privacyMode
            onFp6UnlockRequested: {
                panel.indicators.hide();
                if (greeter) greeter.notifyUserRequestedApp();
            }
            onFp6ScreenshotRequested: {
                panel.indicators.hide();
                fp6ScreenshotDelay.start();
            }
            onFp6AppRequested: {
                panel.indicators.hide();
                shell.activateApplication(appId);
            }
""")
sub("qml/Shell.qml",
"""        Notifications {
            id: notifications

            model: NotificationBackend.Model""",
"""        // FP6 quick settings: notifications listed in the pulled-down panel
        ListModel { id: fp6NotificationShade }
        Timer {
            id: fp6ScreenshotDelay
            interval: 450 // panel hide animation
            onTriggered: itemGrabber.capture(shell)
        }

        Notifications {
            id: notifications
            fp6Shade: fp6NotificationShade

            model: NotificationBackend.Model""")
print("qs edits ok")
