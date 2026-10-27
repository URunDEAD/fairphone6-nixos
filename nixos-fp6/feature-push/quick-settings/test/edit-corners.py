import sys
root = sys.argv[1]
def sub(path, old, new, count=1):
    p = f"{root}/{path}"
    s = open(p).read()
    n = s.count(old)
    if n != count:
        sys.exit(f"{path}: expected {count} of {old!r}, found {n}")
    open(p, "w").write(s.replace(old, new))

# ---- PanelMenu.qml: bar insets, full-width bar background
sub("qml/Panel/PanelMenu.qml",
"""    property real dragOverFlowWidth: overFlowWidth // FP6: camera cutout
""",
"""    property real dragOverFlowWidth: overFlowWidth // FP6: camera cutout
    // FP6: rounded display corners. The bar row keeps this far from the left/right
    // screen edge (0 when that side of the menu is not at a screen edge).
    property real fp6SafeLeft: 0
    property real fp6SafeRight: 0
""")
sub("qml/Panel/PanelMenu.qml",
"""    Rectangle {
        anchors.fill: bar
        color: panelColor""",
"""    Rectangle {
        // FP6: the bar is inset from rounded corners; its background is not.
        anchors { top: bar.top; bottom: bar.bottom; left: parent.left; right: parent.right }
        color: panelColor""")
sub("qml/Panel/PanelMenu.qml",
"""        objectName: "indicatorsBar"

        anchors {
            left: parent.left
            right: parent.right
        }""",
"""        objectName: "indicatorsBar"

        anchors {
            left: parent.left
            right: parent.right
            leftMargin: root.fp6SafeLeft
            rightMargin: root.fp6SafeRight
        }""")

# ---- Panel.qml: inset computation and use
sub("qml/Panel/Panel.qml",
"""    property real menuWidth: partialWidth ? units.gu(40) : width
""",
"""    // FP6: rounded display corners (Android: rounded_corner_radius + content
    // padding). The radius is in physical pixels (module option); the inset is
    // how far from the side edge content must start so that its top-left/right
    // corner (at `top` below the screen edge) is inside the arc.
    readonly property real fp6CornerRadius: @fp6CornerRadiusPx@ / Screen.devicePixelRatio
    function fp6CornerInset(top) {
        var r = fp6CornerRadius;
        if (r <= 0 || top >= r) return 0;
        var dy = r - top;
        return Math.ceil(r - Math.sqrt(r * r - dy * dy));
    }
    // collapsed row: icons are 2 gu high, centred in the bar
    readonly property real fp6SafeInset: fp6CornerInset((minimizedPanelHeight - units.gu(2)) / 2)
    // expanded row: icon + label start about 1 gu below the edge
    readonly property real fp6ExpandedSafeInset: fp6CornerInset(units.gu(1))
    property real menuWidth: partialWidth ? units.gu(40) : width
""")
sub("qml/Panel/Panel.qml",
"""                leftMargin: units.gu(1)
                right: __indicators.left""",
"""                leftMargin: Math.max(units.gu(1), root.fp6SafeInset) // FP6: rounded corner
                right: __indicators.left""")
# application menus (left aligned)
sub("qml/Panel/Panel.qml",
"""            x: menuContentX
            model: registeredMenuModel.model
            width: root.menuWidth
            overFlowWidth: width""",
"""            x: menuContentX
            model: registeredMenuModel.model
            width: root.menuWidth
            overFlowWidth: width
            fp6SafeLeft: x <= 0 ? (expanded ? root.fp6ExpandedSafeInset : root.fp6SafeInset) : 0
            fp6SafeRight: x + width >= root.width ? (expanded ? root.fp6ExpandedSafeInset : root.fp6SafeInset) : 0""")
sub("qml/Panel/Panel.qml",
"""            overFlowWidth: Math.min(width - appMenuClear, root.cameraCutoutRightWidth)
            dragOverFlowWidth: Math.min(width - appMenuClear, Math.max(overFlowWidth, root.cameraCutoutExtent))""",
"""            // FP6: overFlowWidth is measured from the (corner-inset) bar's right edge,
            // dragOverFlowWidth from the screen edge.
            fp6SafeLeft: x <= 0 ? (expanded ? root.fp6ExpandedSafeInset : root.fp6SafeInset) : 0
            fp6SafeRight: x + width >= root.width ? (expanded ? root.fp6ExpandedSafeInset : root.fp6SafeInset) : 0
            overFlowWidth: Math.min(width - appMenuClear - fp6SafeLeft, root.cameraCutoutRightWidth) - fp6SafeRight
            dragOverFlowWidth: Math.min(width - appMenuClear, Math.max(overFlowWidth, root.cameraCutoutExtent) + fp6SafeRight)""")
sub("qml/Panel/Panel.qml",
"""      readonly property real fp6LeftLimit: __indicators.width - __indicators.appMenuClear
          - units.gu(1) - (root.screenRecording ? units.gu(2.5) : 0)""",
"""      readonly property real fp6LeftLimit: __indicators.width - __indicators.appMenuClear
          - Math.max(units.gu(1), __indicators.fp6SafeLeft) - __indicators.fp6SafeRight
          - (root.screenRecording ? units.gu(2.5) : 0)""")
print("corner edits ok")
