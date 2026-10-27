#!/usr/bin/env python3
"""Edits a fully patched Lomiri tree (every other module's patches applied,
see test/mkpatch.sh) for the FP6 accessibility module. mkpatch.sh diffs the
result into ../lomiri-accessibility.patch."""
import sys
root = sys.argv[1]

def sub(path, old, new, count=1):
    p = f"{root}/{path}"
    s = open(p).read()
    n = s.count(old)
    if n != count:
        sys.exit(f"{path}: expected {count} of {old!r}, found {n}")
    open(p, "w").write(s.replace(old, new))

# ---------------- ShellScreen: magnification transform, magnifier, colour matrix
sub("qml/ShellScreen.qml",
"""import FP6.DisplayComfort 1.0 as FP6DisplayComfort
""",
"""import FP6.DisplayComfort 1.0 as FP6DisplayComfort
import FP6.Accessibility 1.0 as FP6A11y
""")
sub("qml/ShellScreen.qml",
"""    Loader {
        id: loader
        width: screenWindow.width
        height: screenWindow.height
""",
"""    Loader {
        id: loader
        width: screenWindow.width
        height: screenWindow.height
        // fp6 accessibility: screen magnification of the whole scene (apps,
        // panel, launcher, OSK, lock screen). Identity while not zoomed.
        transform: [
            Scale { xScale: fp6Magnifier.zoom; yScale: fp6Magnifier.zoom },
            Translate { x: -fp6Magnifier.originX * fp6Magnifier.zoom; y: -fp6Magnifier.originY * fp6Magnifier.zoom }
        ]
""")
sub("qml/ShellScreen.qml",
"""        greyscale: fp6DisplayComfortSettings.greyscaleActive ? 1 : 0
        Behavior on greyscale { NumberAnimation { duration: 1000; easing.type: Easing.InOutQuad } }
    }
""",
"""        greyscale: fp6DisplayComfortSettings.greyscaleActive ? 1 : 0
        Behavior on greyscale { NumberAnimation { duration: 1000; easing.type: Easing.InOutQuad } }
        // fp6 accessibility: inversion / colour correction, applied before
        // greyscale and night light in the same pass (empty = none).
        preMatrix: fp6A11yColour.matrix
    }

    // fp6 accessibility (Settings > Accessibility, com.fairphone6.accessibility)
    GSettings {
        id: fp6A11ySettings
        schema.id: "com.fairphone6.accessibility"
    }
    FP6A11y.ColourMatrix {
        id: fp6A11yColour
        inversion: fp6A11ySettings.colourInversion === true
        correction: fp6A11ySettings.colourCorrection || "none"
        strength: fp6A11ySettings.colourCorrectionStrength !== undefined ? fp6A11ySettings.colourCorrectionStrength : 1
    }
    // Gestures and the view of the magnification (the Loader's transform).
    // No event filter while every trigger is off and the screen is not zoomed.
    FP6A11y.Magnifier {
        id: fp6Magnifier
        objectName: "fp6Magnifier"
        anchors.fill: parent
        tapEnabled: fp6A11ySettings.magnificationTap === true
        tapCount: fp6A11ySettings.magnificationTapCount || 3
        holdEnabled: fp6A11ySettings.magnificationHold !== false
        keyShortcut: fp6A11ySettings.magnificationShortcut === true
        level: fp6A11ySettings.magnificationLevel || 2
        slop: units.gu(1)
        tapDistance: units.gu(10)
        onLevelCommitted: fp6A11ySettings.magnificationLevel = level
    }
    // While magnified: an outline around the screen (as on Android).
    Rectangle {
        anchors.fill: parent
        z: 999999
        visible: fp6Magnifier.zoomed
        color: "transparent"
        border.width: units.dp(3)
        border.color: "#E95420"
    }
""")

# ---------------- quick settings: Colour inversion and Magnification tiles
sub("qml/Panel/QuickSettings/QsBackend.qml",
"""import QMenuModel 1.0
""",
"""import QMenuModel 1.0
import FP6.Accessibility 1.0 as FP6A11y
""")
sub("qml/Panel/QuickSettings/QsBackend.qml",
"""    signal screenshotRequested()
""",
"""    signal screenshotRequested()
    // FP6 accessibility: close the panel (before the magnification starts)
    signal fp6HideRequested()
""")
sub("qml/Panel/QuickSettings/QsBackend.qml",
"""    GSettings { id: notificationSettings; schema.id: "com.fairphone6.notifications" }
""",
"""    GSettings { id: notificationSettings; schema.id: "com.fairphone6.notifications" }
    GSettings { id: a11ySettings; schema.id: "com.fairphone6.accessibility" }
""")
sub("qml/Panel/QuickSettings/QsBackend.qml",
"""greyscale, scanQr]""",
"""greyscale, scanQr, colourInversion, magnification]""")
sub("qml/Panel/QuickSettings/QsBackend.qml",
"""    // ---- page tiles: the indicator pages that have no switch tile of their own
""",
"""    // ---- FP6 accessibility (Settings > Accessibility)
    QsTileDef {
        id: colourInversion
        tileId: "colour-inversion"; label: i18n.tr("Colour inversion")
        iconOn: "display-brightness-symbolic"
        available: a11ySettings.colourInversion !== undefined
        active: !!a11ySettings.colourInversion
        canWide: false
        settingsUrl: "settings:///system/accessibility"
        toggle: function() { a11ySettings.colourInversion = !a11ySettings.colourInversion; }
    }
    QsTileDef {
        id: magnification
        tileId: "magnification"; label: i18n.tr("Magnification")
        iconOn: "zoom-out"; iconOff: "zoom-in"
        available: FP6A11y.MagnifierControl.available
        active: FP6A11y.MagnifierControl.zoomed
        canWide: false
        settingsUrl: "settings:///system/accessibility"
        // zoom in at the screen centre once the panel is closed / zoom out
        toggle: function() {
            var zoomed = FP6A11y.MagnifierControl.zoomed;
            backend.fp6HideRequested();
            FP6A11y.MagnifierControl.toggle(zoomed ? 0 : 450);
        }
    }

    // ---- page tiles: the indicator pages that have no switch tile of their own
""")
sub("qml/Panel/Panel.qml",
"""            onScreenshotRequested: root.fp6ScreenshotRequested()
""",
"""            onScreenshotRequested: root.fp6ScreenshotRequested()
            onFp6HideRequested: __indicators.hide()
""")
print("edited", root)
