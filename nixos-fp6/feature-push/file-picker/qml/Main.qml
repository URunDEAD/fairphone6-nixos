// FP6 file picker (Lomiri UI Toolkit 1.3, Suru). Started by
// xdg-desktop-portal-lomiri for OpenFile / SaveFile / SaveFiles requests;
// `picker` (main.cpp) carries the request and takes the answer.
import QtQuick 2.12
import Qt.labs.folderlistmodel 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import "Util.js" as Util

MainView {
    id: root
    applicationName: "fp6-file-picker"
    width: units.gu(45)
    height: units.gu(80)
    anchorToKeyboard: true

    readonly property var req: picker.request
    readonly property string mode: req.mode || "open"          // open | folder | save | save-files
    readonly property bool saving: mode === "save" || mode === "save-files"
    readonly property bool pickFolder: mode === "folder" || mode === "save-files"
    readonly property bool multiple: !!req.multiple && mode === "open"
    readonly property var filters: req.filters || []
    readonly property var places: req.places || []
    readonly property bool contentHub: !!req.contentHub && mode === "open"
    readonly property string home: req.home || "/"
    property int filterIndex: (req.currentFilter !== undefined && req.currentFilter < filters.length) ? req.currentFilter : -1
    readonly property var filter: filterIndex >= 0 ? filters[filterIndex] : null
    readonly property var filterRegex: filter && filter.globs.indexOf("*") < 0 ? new RegExp(filter.regex, "i") : null
    property bool showHidden: false
    property string folder: ""
    // "recent", "folder" or "apps"
    property string view: "folder"
    property var selection: []
    property var choiceValues: ({})

    readonly property string titleText: req.title ? req.title
        : mode === "save" ? i18n.tr("Save File")
        : mode === "save-files" ? i18n.tr("Save Files")
        : mode === "folder" ? i18n.tr("Select Folder")
        : multiple ? i18n.tr("Open Files") : i18n.tr("Open File")
    readonly property string acceptText: req.acceptLabel ? req.acceptLabel
        : saving ? i18n.tr("Save")
        : pickFolder ? i18n.tr("Select")
        : i18n.tr("Open")

    // Sections: Recent (open only), the places, Other apps (content-hub).
    readonly property var sectionItems: {
        var s = [];
        if (mode === "open")
            s.push({ id: "recent", name: i18n.tr("Recent") });
        for (var i = 0; i < places.length; i++)
            s.push({ id: "place", name: places[i].name, path: places[i].path });
        if (contentHub)
            s.push({ id: "apps", name: i18n.tr("Other apps") });
        return s;
    }

    function finish(paths) {
        var cv = [];
        for (var k in choiceValues)
            cv.push([k, choiceValues[k]]);
        picker.finish({ response: 0, paths: paths, filter: filterIndex, choices: cv });
    }
    function cancel() { picker.finish({ response: 1 }); }

    function openFolder(path) {
        view = "folder";
        folder = path;
        syncSection();
    }
    // Highlight the place that contains the current folder.
    function syncSection() {
        var best = -1, bestLen = -1;
        for (var i = 0; i < sectionItems.length; i++) {
            var s = sectionItems[i];
            if (view === "recent" && s.id === "recent") { best = i; break; }
            if (view === "apps" && s.id === "apps") { best = i; break; }
            if (view === "folder" && s.id === "place" && Util.isInside(folder, s.path) && s.path.length > bestLen) {
                best = i; bestLen = s.path.length;
            }
        }
        if (best >= 0 && sections.selectedIndex !== best)
            sections.selectedIndex = best;
    }
    function placePath(id) {
        for (var i = 0; i < places.length; i++)
            if (places[i].id === id) return places[i].path;
        return "";
    }

    function isSelected(path) { return selection.indexOf(path) >= 0; }
    function toggle(path) {
        var s = selection.slice();
        var i = s.indexOf(path);
        if (i >= 0) s.splice(i, 1); else s.push(path);
        selection = s;
    }

    function activateFile(path, name) {
        if (saving) {
            nameField.text = name;
            return;
        }
        if (multiple)
            toggle(path);
        else
            finish([path]);
    }

    function accept() {
        if (multiple) {
            if (selection.length > 0) finish(selection);
        } else if (pickFolder) {
            finish([folder]);
        } else if (mode === "save") {
            var name = nameField.text.trim();
            if (name === "" || name.indexOf("/") >= 0) return;
            if (!picker.isWritableDir(folder)) {
                PopupUtils.open(messageDialog, root, { text: i18n.tr("This folder is read-only. Choose another folder.") });
                return;
            }
            var path = picker.join(folder, name);
            if (picker.isDir(path)) { openFolder(path); nameField.text = ""; return; }
            if (picker.exists(path))
                PopupUtils.open(replaceDialog, root, { path: path, name: name });
            else
                finish([path]);
        }
    }

    Component.onCompleted: {
        refillRecent();
        var start = req.currentFolder && picker.isDir(req.currentFolder) ? req.currentFolder : "";
        if (start !== "") {
            openFolder(start);
        } else if (mode === "open" && recentModel.count > 0) {
            view = "recent";
            syncSection();
        } else {
            openFolder(placePath("downloads") || placePath("documents") || home);
        }
        if (req.currentName)
            nameField.text = req.currentName;
        var cv = {};
        var cs = req.choices || [];
        for (var i = 0; i < cs.length; i++)
            cv[cs[i].id] = cs[i].selected;
        choiceValues = cv;
        previewTimer.start();
    }

    // Offline screenshots (test/preview.sh): FP6_PREVIEW=<step>[,<step>...]
    // Offline screenshots/tests (test/run-tests.sh): FP6_PREVIEW=<step>,<step>
    // One step per tick, so the folder model can load between steps.
    property var previewSteps: picker.preview === "" ? [] : picker.preview.split(",")
    property int previewStep: 0
    Timer {
        id: previewTimer
        interval: 700
        repeat: true
        onTriggered: {
            if (root.previewStep >= root.previewSteps.length) { stop(); return; }
            var st = root.previewSteps[root.previewStep++];
            if (st.indexOf("place=") === 0) openFolder(placePath(st.substring(6)));
            else if (st.indexOf("folder=") === 0) openFolder(st.substring(7));
            else if (st === "recent") { view = "recent"; syncSection(); }
            else if (st === "apps") { view = "apps"; syncSection(); }
            else if (st === "select2") {
                var s = [];
                for (var j = 0; j < fsModel.count && s.length < 2; j++)
                    if (!fsModel.get(j, "fileIsDir")) s.push(fsModel.get(j, "filePath"));
                selection = s;
            }
            else if (st === "filters") PopupUtils.open(filterPopover, filterAnchor);
            else if (st === "newfolder") PopupUtils.open(newFolderDialog, root);
            else if (st.indexOf("name=") === 0) nameField.text = st.substring(5);
            else if (st === "replace" || st === "accept") accept();
            else if (st.indexOf("pick=") === 0) {
                var pp = st.substring(5);
                activateFile(pp, pp.substring(pp.lastIndexOf("/") + 1));
            }
            else if (st === "cancel") cancel();
        }
    }

    ListModel { id: recentModel }
    function refillRecent() {
        recentModel.clear();
        var r = req.recent || [];
        for (var i = 0; i < r.length; i++) {
            if (filterRegex && !filterRegex.test(r[i].name)) continue;
            if (!showHidden && r[i].name.charAt(0) === ".") continue;
            recentModel.append(r[i]);
        }
    }
    onFilterIndexChanged: refillRecent()

    FolderListModel {
        id: fsModel
        folder: Util.fileUrl(root.folder !== "" ? root.folder : root.home)
        showDirsFirst: true
        showDotAndDotDot: false
        showHidden: root.showHidden
        showFiles: !root.pickFolder
        caseSensitive: false
        sortField: FolderListModel.Name
        nameFilters: root.filter && root.filter.globs.indexOf("*") < 0 ? root.filter.globs : []
    }

    Page {
        id: page
        anchors.fill: parent

        header: PageHeader {
            id: header
            title: root.titleText
            leadingActionBar.actions: [
                Action {
                    iconName: "close"
                    text: i18n.tr("Cancel")
                    onTriggered: root.cancel()
                }
            ]
            trailingActionBar.actions: [
                Action {
                    iconName: "filters"
                    text: i18n.tr("File type")
                    visible: root.filters.length > 1 && root.view !== "apps"
                    onTriggered: PopupUtils.open(filterPopover, filterAnchor)
                },
                Action {
                    iconName: "add"
                    text: i18n.tr("New folder")
                    visible: (root.saving || root.pickFolder) && root.view === "folder"
                    onTriggered: PopupUtils.open(newFolderDialog, root)
                }
            ]
            extension: Sections {
                id: sections
                anchors { left: parent.left; leftMargin: units.gu(1); bottom: parent.bottom }
                model: root.sectionItems.map(function (s) { return s.name; })
                onSelectedIndexChanged: {
                    var s = root.sectionItems[selectedIndex];
                    if (!s) return;
                    if (s.id === "recent") root.view = "recent";
                    else if (s.id === "apps") root.view = "apps";
                    else if (!(root.view === "folder" && Util.isInside(root.folder, s.path) && root.folder.length >= s.path.length
                               && root.bestPlaceFor(root.folder) === selectedIndex))
                        root.openFolder(s.path);
                }
            }
        }

        // Path row (folder view) / caption (recent): up button, location, filter.
        Rectangle {
            id: pathRow
            anchors { top: header.bottom; left: parent.left; right: parent.right }
            height: root.view === "apps" ? 0 : units.gu(5)
            visible: height > 0
            color: theme.palette.normal.background

            AbstractButton {
                id: upButton
                anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
                width: root.view === "folder" ? units.gu(5) : 0
                visible: width > 0
                enabled: root.folder !== "/"
                onClicked: root.openFolder(picker.parentOf(root.folder))
                Icon {
                    anchors.centerIn: parent
                    width: units.gu(2.5); height: width
                    name: "go-up"
                    color: upButton.enabled ? theme.palette.normal.backgroundText : theme.palette.disabled.backgroundText
                }
            }
            Label {
                anchors {
                    left: upButton.right; leftMargin: root.view === "folder" ? 0 : units.gu(2)
                    right: filterAnchor.left; rightMargin: units.gu(1)
                    verticalCenter: parent.verticalCenter
                }
                text: root.view === "recent"
                      ? (req.appName ? i18n.tr("Recent files · for %1").arg(req.appName) : i18n.tr("Recent files"))
                      : Util.prettyPath(root.folder, root.home)
                elide: Text.ElideLeft
                textSize: Label.Small
                color: theme.palette.normal.backgroundSecondaryText
            }
            AbstractButton {
                id: filterAnchor
                anchors { right: parent.right; rightMargin: units.gu(1); top: parent.top; bottom: parent.bottom }
                width: root.filter ? filterLabel.implicitWidth + units.gu(3) : 0
                visible: width > 0
                enabled: root.filters.length > 1
                onClicked: PopupUtils.open(filterPopover, filterAnchor)
                Label {
                    id: filterLabel
                    anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                    text: root.filter ? root.filter.name + (root.filters.length > 1 ? " ▾" : "") : ""
                    textSize: Label.Small
                    color: theme.palette.normal.activity
                    width: Math.min(implicitWidth, units.gu(18))
                    elide: Text.ElideRight
                }
            }
            Rectangle {
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: units.dp(1)
                color: theme.palette.normal.base
            }
        }

        // Folder view
        ListView {
            id: folderList
            anchors { top: pathRow.bottom; left: parent.left; right: parent.right; bottom: bottomBar.top }
            visible: root.view === "folder"
            clip: true
            model: fsModel
            delegate: EntryItem {
                name: fileName
                path: filePath
                isDir: fileIsDir
                subtitle: fileIsDir ? "" : Util.fmtSize(fileSize) + " · " + Util.fmtDate(fileModified)
                checkable: root.multiple && !fileIsDir
                checked: root.multiple && root.isSelected(filePath)
                onActivated: fileIsDir ? root.openFolder(filePath) : root.activateFile(filePath, fileName)
            }
            Label {
                anchors.centerIn: parent
                width: parent.width - units.gu(6)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                visible: fsModel.count === 0 && fsModel.status === FolderListModel.Ready
                text: root.pickFolder ? i18n.tr("No folders here") : root.filter ? i18n.tr("No matching files here") : i18n.tr("This folder is empty")
                color: theme.palette.normal.backgroundSecondaryText
            }
            Scrollbar { flickableItem: folderList; align: Qt.AlignTrailing }
        }

        // Recent view
        ListView {
            id: recentList
            anchors { top: pathRow.bottom; left: parent.left; right: parent.right; bottom: bottomBar.top }
            visible: root.view === "recent"
            clip: true
            model: recentModel
            delegate: EntryItem {
                name: model.name
                path: model.path
                isDir: false
                subtitle: Util.prettyPath(model.folder, root.home) + " · " + Util.fmtDate(new Date(model.mtime * 1000))
                checkable: root.multiple
                checked: root.multiple && root.isSelected(model.path)
                onActivated: root.activateFile(model.path, model.name)
            }
            Label {
                anchors.centerIn: parent
                width: parent.width - units.gu(6)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                visible: recentModel.count === 0
                text: i18n.tr("No recent files")
                color: theme.palette.normal.backgroundSecondaryText
            }
            Scrollbar { flickableItem: recentList; align: Qt.AlignTrailing }
        }

        // Other apps (content-hub peer picker), loaded on demand so a
        // missing Lomiri.Content module only disables this section.
        Loader {
            id: appsLoader
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            active: root.view === "apps"
            visible: active
            source: "OtherApps.qml"
            onStatusChanged: if (status === Loader.Error) appsError.visible = true
        }
        Label {
            id: appsError
            visible: false
            anchors.centerIn: appsLoader
            text: i18n.tr("Other apps are not available.")
        }
        Connections {
            target: appsLoader.item
            ignoreUnknownSignals: true
            onPicked: root.finish(paths)
            onCanceled: { root.view = "recent"; root.syncSection(); }
        }

        // Choices, the name field (save) and the accept button.
        Rectangle {
            id: bottomBar
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            readonly property bool needed: root.view !== "apps" && (root.multiple || root.pickFolder || root.saving || choicesRepeater.count > 0)
            height: needed ? barColumn.height + units.gu(2) : 0
            visible: needed
            color: theme.palette.normal.background
            Rectangle {
                anchors { left: parent.left; right: parent.right; top: parent.top }
                height: units.dp(1)
                color: theme.palette.normal.base
            }
            Column {
                id: barColumn
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1); leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(1)

                Repeater {
                    id: choicesRepeater
                    model: req.choices || []
                    delegate: Item {
                        width: barColumn.width
                        height: modelData.options.length === 0 ? units.gu(4) : units.gu(4)
                        Label {
                            anchors { left: parent.left; verticalCenter: parent.verticalCenter; right: choiceControl.left; rightMargin: units.gu(1) }
                            text: modelData.label
                            elide: Text.ElideRight
                        }
                        Item {
                            id: choiceControl
                            anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                            width: modelData.options.length === 0 ? choiceCheck.width : choiceButton.width
                            height: units.gu(4)
                            CheckBox {
                                id: choiceCheck
                                anchors.verticalCenter: parent.verticalCenter
                                visible: modelData.options.length === 0
                                checked: root.choiceValues[modelData.id] === "true"
                                onTriggered: {
                                    var cv = root.choiceValues; cv[modelData.id] = checked ? "true" : "false"; root.choiceValues = cv;
                                }
                            }
                            Button {
                                id: choiceButton
                                anchors.verticalCenter: parent.verticalCenter
                                visible: modelData.options.length > 0
                                width: units.gu(18)
                                text: Util.optionLabel(modelData.options, root.choiceValues[modelData.id])
                                onClicked: PopupUtils.open(choicePopover, choiceButton, { choice: modelData })
                            }
                        }
                    }
                }

                TextField {
                    id: nameField
                    visible: root.mode === "save"
                    width: parent.width
                    placeholderText: i18n.tr("File name")
                    inputMethodHints: Qt.ImhNoPredictiveText
                    onAccepted: root.accept()
                }

                Row {
                    visible: root.multiple || root.pickFolder || root.saving
                    anchors.right: parent.right
                    spacing: units.gu(1)
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        visible: root.multiple
                        text: i18n.tr("%1 selected").arg(root.selection.length)
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                    Button {
                        text: root.acceptText
                        color: theme.palette.normal.positive
                        enabled: root.multiple ? root.selection.length > 0
                               : root.mode === "save" ? nameField.text.trim().length > 0 && root.view === "folder"
                               : root.view === "folder"
                        onClicked: root.accept()
                    }
                }
            }
        }
    }

    function bestPlaceFor(path) {
        var best = -1, bestLen = -1;
        for (var i = 0; i < sectionItems.length; i++) {
            var s = sectionItems[i];
            if (s.id === "place" && Util.isInside(path, s.path) && s.path.length > bestLen) { best = i; bestLen = s.path.length; }
        }
        return best;
    }

    Component {
        id: filterPopover
        ActionSelectionPopover {
            actions: ActionList {
                id: filterActions
            }
            Component.onCompleted: {
                var list = [];
                for (var i = 0; i < root.filters.length; i++) {
                    list.push(filterAction.createObject(filterActions, { text: (i === root.filterIndex ? "✓ " : "    ") + root.filters[i].name, idx: i }));
                }
                filterActions.actions = list;
            }
        }
    }
    Component {
        id: filterAction
        Action {
            property int idx
            onTriggered: root.filterIndex = idx
        }
    }

    Component {
        id: choicePopover
        ActionSelectionPopover {
            property var choice
            actions: ActionList { id: choiceActions }
            Component.onCompleted: {
                var list = [];
                for (var i = 0; i < choice.options.length; i++)
                    list.push(choiceAction.createObject(choiceActions, { text: choice.options[i].label, cid: choice.id, oid: choice.options[i].id }));
                choiceActions.actions = list;
            }
        }
    }
    Component {
        id: choiceAction
        Action {
            property string cid
            property string oid
            onTriggered: { var cv = root.choiceValues; cv[cid] = oid; root.choiceValues = cv; }
        }
    }

    Component {
        id: newFolderDialog
        Dialog {
            id: nfd
            title: i18n.tr("New folder")
            TextField {
                id: nfName
                placeholderText: i18n.tr("Folder name")
                inputMethodHints: Qt.ImhNoPredictiveText
                Component.onCompleted: forceActiveFocus()
                onAccepted: createButton.clicked()
            }
            Label {
                id: nfError
                visible: text !== ""
                color: theme.palette.normal.negative
                wrapMode: Text.WordWrap
            }
            Button {
                id: createButton
                text: i18n.tr("Create")
                color: theme.palette.normal.positive
                enabled: nfName.text.trim() !== "" && nfName.text.indexOf("/") < 0
                onClicked: {
                    var p = picker.join(root.folder, nfName.text.trim());
                    if (picker.exists(p)) { nfError.text = i18n.tr("A file or folder with this name already exists."); return; }
                    if (!picker.mkdir(p)) { nfError.text = i18n.tr("The folder could not be created here."); return; }
                    PopupUtils.close(nfd);
                    root.openFolder(p);
                }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(nfd)
            }
        }
    }

    Component {
        id: replaceDialog
        Dialog {
            id: rd
            property string path
            property string name
            title: i18n.tr("Replace “%1”?").arg(name)
            text: i18n.tr("A file with this name already exists in this folder. Replacing it overwrites its contents.")
            Button {
                text: i18n.tr("Replace")
                color: theme.palette.normal.negative
                onClicked: { PopupUtils.close(rd); root.finish([rd.path]); }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(rd)
            }
        }
    }

    Component {
        id: messageDialog
        Dialog {
            id: md
            title: root.titleText
            Button {
                text: i18n.tr("OK")
                onClicked: PopupUtils.close(md)
            }
        }
    }

    onShowHiddenChanged: refillRecent()
}
