#!/usr/bin/env python3
# Applies the spread-close-all edits to a copy of the full target's patched
# Lomiri tree (all other modules' patches applied); test/mkpatch.sh turns the
# result into ../lomiri-spread-close-all.patch.
#   edit-spread-close-all.py TREE
import sys

root = sys.argv[1]


def sub(path, old, new, count=1):
    p = f"{root}/{path}"
    s = open(p).read()
    n = s.count(old)
    if n != count:
        sys.exit(f"{path}: expected {count} of {old!r}, found {n}")
    open(p, "w").write(s.replace(old, new))


# ---------------- FloatingFlickable: drag hooks for the overscroll gesture
sub("qml/Components/FloatingFlickable.qml",
"""Item {
    property alias contentWidth: flickable.contentWidth""",
"""Item {
    id: fp6Root
    property alias contentWidth: flickable.contentWidth""")
sub("qml/Components/FloatingFlickable.qml",
"""    property alias dragging: flickable.dragging
""",
"""    property alias dragging: flickable.dragging

    // FP6 close all (spread-close-all): drag hooks for the spread's
    // overscroll-at-the-start gesture. fp6DragStarted comes before the inner
    // Flickable gets its press, so boundsBehavior can still be changed.
    property alias boundsBehavior: flickable.boundsBehavior
    readonly property alias moving: flickable.moving
    readonly property alias fp6Pressed: swipeArea.pressed
    readonly property alias fp6TouchPosition: swipeArea.touchPosition
    signal fp6DragStarted(point position)
    signal fp6DragEnded(point position)
""")
sub("qml/Components/FloatingFlickable.qml",
"""        onDraggingChanged: dragging ? mouseEventGenerator.press(touchPosition)
                                    : mouseEventGenerator.release(touchPosition)""",
"""        onDraggingChanged: {
            if (dragging) {
                fp6Root.fp6DragStarted(touchPosition);
                mouseEventGenerator.press(touchPosition);
            } else {
                mouseEventGenerator.release(touchPosition);
                fp6Root.fp6DragEnded(touchPosition);
            }
        }""")

# ---------------- Stage: the signal for the shell
sub("qml/Stage/Stage.qml",
"""    signal itemSnapshotRequested(Item item)
""",
"""    signal itemSnapshotRequested(Item item)

    // FP6 close all apps (spread-close-all): the spread closed all apps; the
    // shell goes home (opens the app drawer like the home button).
    signal fp6CloseAllHome()
""")

# ---------------- Stage: the close-all overlay on top of the spread's flickable
sub("qml/Stage/Stage.qml",
"""                onExited: wasTouchPress = false;
            }
        }
""",
"""                onExited: wasTouchPress = false;
            }

            // FP6 close all apps (Spread/Fp6CloseAll.qml): drag left-to-right
            // past the start of the list (only a drag that starts with the
            // list at its start), or "Clear all".
            Fp6CloseAll {
                id: fp6CloseAll
                objectName: "fp6CloseAll"
                anchors.fill: parent
                spread: spreadItem
                flickable: floatingFlickable
                repeater: appRepeater
                applicationManager: root.applicationManager
                active: root.state == "spread" && root.mode !== "windowed"
                callActive: typeof callManager !== "undefined" && callManager !== null && callManager.hasCalls === true
                onFinished: {
                    spreadItem.highlightedIndex = -1;
                    priv.goneToSpread = false;
                    root.fp6CloseAllHome();
                }
            }
        }
""")

# ---------------- Stage: closable cards follow the drag and fade
sub("qml/Stage/Stage.qml",
"""                        PropertyChanges {
                            target: appDelegate
                            x: spreadMaths.targetX
                            y: spreadMaths.targetY
                            z: index
                            height: spreadItem.spreadItemHeight
                            visible: spreadMaths.itemVisible
                        }""",
"""                        PropertyChanges {
                            target: appDelegate
                            // FP6 close all: the cards follow the drag, the closable ones fade
                            x: spreadMaths.targetX + fp6CloseAll.offset
                            y: spreadMaths.targetY
                            z: index
                            height: spreadItem.spreadItemHeight
                            visible: spreadMaths.itemVisible
                            opacity: fp6CloseAll.isClosable(appDelegate.appId) ? 1 - fp6CloseAll.fade : 1
                        }""")

# ---------------- Shell: home after close all
sub("qml/Shell.qml",
"""            onSpreadShownChanged: {
                panel.indicators.hide();
                panel.applicationMenus.hide();
            }
""",
"""            onSpreadShownChanged: {
                panel.indicators.hide();
                panel.applicationMenus.hide();
            }
            // FP6 close all apps (spread-close-all)
            onFp6CloseAllHome: shell.showHome()
""")
