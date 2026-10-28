// Previews + checks of Settings > Sound > Lock sounds (offscreen, 46.5x103.5 gu
// at 24 px/gu = 1116x2484). The Sound page is the one the target builds
// (stock + audio-routing's patch + this module), the pickers are the real
// files. Scene in Shot.js; PASS/FAIL lines on stdout.
import QtQuick 2.12
import Lomiri.Components 1.3
import GSettings 1.0
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }

    // LSS context properties the stock page reads
    property bool showAllUI: false
    property bool isSnap: false

    GSettings { id: gs; schema.id: "com.fairphone6.lock-sounds" }

    function check(name, ok, detail) { console.log((ok ? "PASS " : "FAIL ") + Shot.shot + ": " + name + (ok ? "" : " (" + detail + ")")); }

    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("PageComponent.qml")
    }

    function find(item, name) {
        if (!item) return null;
        if (item.objectName === name) return item;
        var kids = item.children || [];
        for (var i = 0; i < kids.length; i++) { var r = find(kids[i], name); if (r) return r; }
        return null;
    }

    property var picker: null
    property var files: null

    Component.onCompleted: {
        if (Shot.shot === "sound-off") gs.enabled = false;
        if (Shot.shot === "picker-custom" || Shot.shot === "sound-custom") gs.lockSound = Shot.home + "/Music/Quiet bell.ogg";
    }

    Timer {
        interval: 1200; running: true
        onTriggered: {
            var page = pageStack.primaryPage;
            var section = mv.find(page, "fp6LockSoundsSection");
            if (Shot.shot.indexOf("sound") === 0) {
                mv.check("section on the Sound page", !!section, "not found");
                var kb = mv.find(page, "keyboardSoundSwitch");
                if (section && kb) {
                    var p1 = kb.mapToItem(null, 0, 0).y, p2 = section.mapToItem(null, 0, 0).y;
                    mv.check("section below Keyboard sound", p2 > p1, p1 + " " + p2);
                }
                var lockItem = mv.find(page, "fp6LockSound");
                if (Shot.shot === "sound") mv.check("lock sound shows its name", lockItem && lockItem.value === "Pop down", lockItem ? lockItem.value : "-");
                if (Shot.shot === "sound-custom") mv.check("custom shows the file name", lockItem && lockItem.value === "Quiet bell", lockItem ? lockItem.value : "-");
                if (Shot.shot === "sound-off") mv.check("pickers disabled when off", lockItem && !lockItem.enabled, "enabled");
                page.flickable.contentY = Math.max(0, section.y - units.gu(30));
                later.start();
                return;
            }
            // open the picker the way the row does
            section.openPicker("lockSound", "Lock sound");
            pickerOpened.start();
        }
    }
    Timer {
        id: pickerOpened; interval: 800
        onTriggered: {
            var top = mv.find(pageStack, "fp6LockSoundPicker");
            mv.picker = top;
            mv.check("picker opened", !!top, "no page");
            if (Shot.shot === "picker") {
                top.choose("click");
                selected.start();
            } else if (Shot.shot === "picker-custom") {
                mv.check("custom entry listed", !!mv.find(top, "fp6LockSoundEntry_custom"), "missing");
                later.start();
            } else { // files, files-music
                pageStack.addPageToNextColumn(top, Qt.resolvedUrl("Fp6LockSoundFiles.qml"), { picker: top, home: Shot.home });
                filesOpened.start();
            }
        }
    }
    Timer {
        id: selected; interval: 300 // GSettings changes arrive from the main loop
        onTriggered: {
            mv.check("tap selects", gs.lockSound === "click", gs.lockSound);
            mv.check("selected entry ticked", mv.picker.current === "click", mv.picker.current);
            later.start();
        }
    }
    Timer {
        id: filesOpened; interval: 800
        onTriggered: {
            var f = mv.find(pageStack, "fp6LockSoundFiles");
            mv.check("files page opened", !!f, "no page");
            if (Shot.shot === "files") { later.start(); return; }
            pageStack.addPageToNextColumn(f, Qt.resolvedUrl("Fp6LockSoundFiles.qml"),
                                          { picker: mv.picker, rootPage: f, home: Shot.home, folder: "file://" + Shot.home + "/Music" });
            musicOpened.start();
        }
    }
    Timer {
        id: musicOpened; interval: 1000
        onTriggered: {
            var row = null;
            // the deepest files page is the Music folder
            mv.check("Music lists the audio files", !!mv.find(pageStack, "fp6LockSoundFile_1"), "fewer than 2 files");
            if (Shot.shot === "files-music") { later.start(); return; }
            // files-choose: pick the first file
            row = mv.find(pageStack, "fp6LockSoundFile_0");
            row.clicked();
            chosen.start();
        }
    }
    Timer {
        id: chosen; interval: 800
        onTriggered: {
            mv.check("file chosen", gs.lockSound === Shot.home + "/Music/A soft tap.wav", gs.lockSound);
            var fp = mv.find(pageStack, "fp6LockSoundFiles"); // may linger until deferred deletion
            mv.check("back on the picker", mv.picker.visible && (!fp || !fp.visible), "files page still shown");
            later.start();
        }
    }
    Timer { id: later; interval: 600; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
