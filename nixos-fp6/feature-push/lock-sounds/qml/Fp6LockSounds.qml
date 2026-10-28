/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * FP6 lock sounds (nixos-fp6/feature-push/lock-sounds), instantiated by
 * Shell.qml with the greeter state and the call state.
 *
 *  * Lock: the display is turned off by the power key or the screen
 *    timeout while the session is unlocked (the sound marks the moment
 *    the lock is set; with privacy-lock's grace period the lock screen
 *    itself comes later), or the lock screen appears with the display on
 *    (Lock from the session menu). Not for proximity, not during calls.
 *  * Unlock: the lock screen goes away (passcode, fingerprint or swipe;
 *    greeter.active false) with the display on.
 *  * Never with Lock sounds off, in Silent Mode, with Do Not Disturb on,
 *    during a call, in the first-run wizard or the LightDM greeter session,
 *    nor for the lock screen shown at session start.
 *  * Latency: the sound that can come next is loaded while the display is
 *    on (lock sound while unlocked, unlock sound while the lock screen is
 *    up) and released afterwards, so there is no PulseAudio stream while
 *    the display is off and play() needs no decoding.
 */
import QtQuick 2.15
import GSettings 1.0
import Powerd 0.1
import QMenuModel 1.0
import "Fp6LockSoundCatalog.js" as Catalog

Item {
    id: root

    property bool greeterActive: false
    property bool inCall: false
    property bool suppressed: false   // wizard, greeter-only session, secondary screen

    readonly property bool silentMode: soundIndicator.silentMode.valid && soundIndicator.silentMode.state === true
    readonly property bool dndActive: @fp6LockSoundsDnd@
    readonly property bool quiet: !settings.enabled || silentMode || dndActive || inCall || suppressed

    readonly property string lockPath: Catalog.resolve(settings.lockSound, settings.preparedFiles)
    readonly property string unlockPath: Catalog.resolve(settings.unlockSound, settings.preparedFiles)

    // For tests and the journal.
    signal soundPlayed(string kind, string path)
    property alias lockPlayer: lockPlayer
    property alias unlockPlayer: unlockPlayer
    property int startupDelay: 5000

    GSettings {
        id: settings
        schema.id: "com.fairphone6.lock-sounds"
    }
    @fp6LockSoundsDndSettings@

    QDBusActionGroup {
        id: soundIndicator
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.sound"
        objectPath: "/org/ayatana/indicator/sound"
        property variant silentMode: action("silent-mode")
        Component.onCompleted: start()
    }

    QtObject {
        id: d
        // Updated in the handlers (after playing), not by bindings, so a
        // player is never released by the very change that plays it.
        property bool displayOn: false      // set in Component.onCompleted
        property bool greeterActive: false
        property bool armed: false
        property bool lockHeld: false
        property bool unlockHeld: false
        property double lastLock: 0
        Component.onCompleted: { displayOn = Powerd.status === Powerd.On; greeterActive = root.greeterActive; }
    }

    // The lock screen of the session start is not a lock.
    Timer {
        interval: root.startupDelay
        running: true
        onTriggered: d.armed = true
    }

    Timer { id: lockHold; interval: 4000; onTriggered: d.lockHeld = false }
    Timer { id: unlockHold; interval: 4000; onTriggered: d.unlockHeld = false }

    Fp6LockSoundPlayer {
        id: lockPlayer
        source: d.lockHeld || (d.armed && d.displayOn && !d.greeterActive && !root.quiet) ? root.lockPath : ""
    }
    Fp6LockSoundPlayer {
        id: unlockPlayer
        source: d.unlockHeld || (d.armed && d.displayOn && d.greeterActive && !root.quiet) ? root.unlockPath : ""
    }

    function playLock() {
        var now = Date.now();
        if (!d.armed || root.quiet || !root.lockPath || now - d.lastLock < 1500) return false;
        d.lastLock = now;
        d.lockHeld = true;
        lockHold.restart();
        lockPlayer.play();
        root.soundPlayed("lock", root.lockPath);
        return true;
    }

    function playUnlock() {
        if (!d.armed || root.quiet || !root.unlockPath) return false;
        d.unlockHeld = true;
        unlockHold.restart();
        unlockPlayer.play();
        root.soundPlayed("unlock", root.unlockPath);
        return true;
    }

    // reason: Powerd.DisplayStateChangeReason
    function displayChanged(on, reason) {
        if (!on && d.displayOn && !d.greeterActive && reason !== Powerd.Proximity)
            playLock();
        d.displayOn = on;
    }

    onGreeterActiveChanged: {
        if (greeterActive && d.displayOn && !d.greeterActive) playLock();
        else if (!greeterActive && d.displayOn && d.greeterActive) playUnlock();
        d.greeterActive = greeterActive;
    }

    Connections {
        target: Powerd
        function onStatusChanged(reason) { root.displayChanged(Powerd.status === Powerd.On, reason) }
    }
}
