/*
 * FP6 quick settings: one tile of the control centre (data only).
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15

QtObject {
    property string tileId
    property string label
    property string iconOn
    property string iconOff: iconOn
    // a live icon (an indicator's own status icon), used instead of iconOn/iconOff
    property url iconSource: ""
    // no switch of its own: tap and hold both open the indicator page
    property bool pageOnly: false
    // the backend exists (indicator running, schema installed, hardware present)
    property bool available: true
    // false: a button (screenshot, scan QR) instead of an on/off switch
    property bool toggleable: true
    property bool active: false
    // shown in a wide tile (second line); "" = "On"/"Off"
    property string subtitle: ""
    property bool canWide: true
    // lock screen: "allow" = works while locked, "unlock" = asks for the passcode first
    property string lockPolicy: "allow"
    // detail (long press / chevron): an indicator page of the panel, else a Settings URL
    property string indicator: ""
    property string settingsUrl: ""
    property var toggle: function() {}
}
