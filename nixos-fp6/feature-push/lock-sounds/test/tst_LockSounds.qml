// Laptop test of the shell side of the lock sounds: the real
// Fp6LockSounds.qml / Fp6LockSoundPlayer.qml / catalog from the target's
// patched Lomiri tree (this module's postPatch applied), the real x86 Lomiri
// Powerd plugin and QMenuModel, on a private bus with test/mock-bus.py
// (repowerd's DisplayPowerStateChange, indicator-sound's silent-mode).
// The greeter and call state are the properties Shell.qml binds.
import QtQuick 2.15
import QtTest 1.0
import GSettings 1.0
import Powerd 0.1
import QMenuModel 1.0

Item {
    id: root
    width: 100; height: 100

    property bool greeter: true    // the session starts on the lock screen
    property bool call: false
    property bool wizard: false

    Fp6LockSounds {
        id: ls
        greeterActive: root.greeter
        inCall: root.call
        suppressed: root.wizard
        startupDelay: 400
    }

    SignalSpy { id: spy; target: ls; signalName: "soundPlayed" }

    QtObject { id: probe; property bool on: Powerd.status === Powerd.On }

    QDBusActionGroup {
        id: ctl
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.sound"
        objectPath: "/org/ayatana/indicator/sound"
        property variant display: action("fp6-test-display")
        property variant silent: action("silent-mode")
        Component.onCompleted: start()
    }

    GSettings { id: gs; schema.id: "com.fairphone6.lock-sounds" }
    GSettings { id: dnd; schema.id: "com.fairphone6.notifications" }

    TestCase {
        id: t
        name: "LockSounds"
        when: windowShown

        // reasons: 1 inactivity, 2 power key, 3 proximity
        function display(on, reason) {
            tryCompare(ctl.display, "valid", true);
            ctl.display.activate((on ? "1" : "0") + ":" + reason);
            tryCompare(probe, "on", on);
            wait(20);
        }
        function expectSounds(n, msg) { wait(80); compare(spy.count, n, msg); }
        function last() { return spy.signalArguments[spy.count - 1]; }
        function bundled(id) { return "@BUNDLE@/" + id + ".wav"; }

        function init() { wait(1600); spy.clear(); } // past the 1.5 s lock debounce
        function cleanup() { root.call = false; root.wizard = false; } // a failed test must not leak

        function test_01_session_start_is_silent() {
            // greeter shown at start, display on: before the startup delay
            // nothing counts; afterwards the first unlock plays.
            compare(ls.quiet, false);
            compare(spy.count, 0);
        }

        function test_02_unlock_preloaded_and_played() {
            compare(ls.unlockPlayer.source, bundled("pop-up"), "unlock sound preloaded while the lock screen is up");
            compare(ls.lockPlayer.source, "", "lock sound not loaded while locked");
            root.greeter = false;
            expectSounds(1);
            compare(last()[0], "unlock");
            compare(last()[1], bundled("pop-up"));
            compare(ls.lockPlayer.source, bundled("pop-down"), "lock sound preloaded while unlocked");
        }

        function test_03_power_key_lock() {
            display(false, 2);
            expectSounds(1);
            compare(last()[0], "lock");
            compare(last()[1], bundled("pop-down"));
            root.greeter = true; // the lock screen arrives with the display off: no second sound
            expectSounds(1);
            wait(4200); // after the hold, nothing stays loaded with the display off
            compare(ls.lockPlayer.source, "");
            compare(ls.unlockPlayer.source, "");
        }

        function test_04_wake_and_unlock() {
            display(true, 2);
            expectSounds(0, "turning the display on is not an unlock");
            compare(ls.unlockPlayer.source, bundled("pop-up"));
            root.greeter = false;
            expectSounds(1);
            compare(last()[0], "unlock");
        }

        function test_05_timeout_lock() {
            display(false, 1);
            expectSounds(1);
            compare(last()[0], "lock");
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            expectSounds(2);
        }

        function test_06_proximity_is_silent() {
            display(false, 3);
            display(true, 3);
            expectSounds(0);
        }

        function test_07_calls_are_silent() {
            root.call = true;
            compare(ls.quiet, true);
            tryCompare(ls.lockPlayer, "source", "", 5000, "no stream during calls (after the 4 s hold)");
            display(false, 2);
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            expectSounds(0);
            root.call = false;
        }

        function test_08_silent_mode() {
            tryCompare(ctl.silent, "valid", true);
            ctl.silent.updateState(true);
            tryCompare(ls, "silentMode", true);
            display(false, 2);
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            expectSounds(0);
            ctl.silent.updateState(false);
            tryCompare(ls, "silentMode", false);
        }

        function test_09_do_not_disturb() {
            dnd.dndActive = true;
            tryCompare(ls, "dndActive", true);
            display(false, 2);
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            expectSounds(0);
            dnd.dndActive = false;
            tryCompare(ls, "dndActive", false);
        }

        function test_10_switched_off() {
            gs.enabled = false;
            tryCompare(ls, "quiet", true);
            display(false, 2);
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            expectSounds(0);
            gs.enabled = true;
            tryCompare(ls, "quiet", false);
        }

        function test_11_lock_with_display_on() {
            // Lock from the session menu: the lock screen appears, display on.
            root.greeter = true;
            expectSounds(1);
            compare(last()[0], "lock");
            display(false, 2); // then the display goes off: no second sound
            expectSounds(1);
            display(true, 2);
            root.greeter = false;
            expectSounds(2);
        }

        function test_12_none_and_choice() {
            gs.lockSound = "";
            gs.unlockSound = "chime";
            tryCompare(ls, "unlockPath", bundled("chime")); // GSettings changes arrive from the main loop
            display(false, 2);
            expectSounds(0, "lock sound None");
            root.greeter = true;
            display(true, 2);
            compare(ls.unlockPlayer.source, bundled("chime"));
            root.greeter = false;
            expectSounds(1);
            compare(last()[1], bundled("chime"));
            gs.lockSound = "pop-down";
            gs.unlockSound = "pop-up";
            tryCompare(ls, "unlockPath", bundled("pop-up"));
        }

        function test_13_custom_files() {
            // Not prepared yet: the file itself (Audio fallback); prepared:
            // the WAV from prepared-files.
            gs.lockSound = "/home/user/Music/My lock.ogg";
            tryCompare(ls, "lockPath", "/home/user/Music/My lock.ogg");
            display(false, 2);
            expectSounds(1);
            compare(ls.lockPlayer.lastBackend, "fallback");
            gs.preparedFiles = ["/home/user/Music/My lock.ogg", "/home/user/.cache/fp6-lock-sounds/0123456789abcdef.wav"];
            tryCompare(ls, "lockPath", "/home/user/.cache/fp6-lock-sounds/0123456789abcdef.wav");
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            gs.lockSound = "pop-down";
            gs.preparedFiles = [];
            tryCompare(ls, "lockPath", bundled("pop-down"));
            spy.clear();
        }

        function test_14_debounce() {
            display(false, 2);
            root.greeter = true;
            display(true, 2);
            root.greeter = false;      // unlock
            display(false, 2);         // lock again within 1.5 s: no second lock sound
            expectSounds(2);
            compare(spy.signalArguments[0][0], "lock");
            compare(spy.signalArguments[1][0], "unlock");
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
        }

        function test_15_wizard_is_silent() {
            root.wizard = true;
            display(false, 2);
            root.greeter = true;
            display(true, 2);
            root.greeter = false;
            expectSounds(0);
            root.wizard = false;
        }
    }
}
