// test/audio-e2e.sh: the real Fp6LockSoundPlayer on a private PipeWire.
// Loads a bundled sound (preloaded SoundEffect) and plays it at fixed
// times, then a custom Ogg file (Audio fallback, cold start), and prints
// the wall-clock time of each play() for the onset measurement.
import QtQuick 2.15
import QtMultimedia 5.6
import "Probe.js" as Probe

Item {
    Fp6LockSoundPlayer { id: p; source: Probe.wav }
    Fp6LockSoundPlayer { id: f; source: "" }
    property int step: 0
    Timer {
        interval: 700; repeat: true; running: true
        onTriggered: {
            step++;
            if (step === 1) console.log("PROBE status", p.ready ? "ready" : "not-ready");
            else if (step >= 2 && step <= 4) { var t = Date.now(); p.play(); console.log("PROBE play effect", t, p.lastBackend); }
            else if (step === 5) { f.source = Probe.ogg; var t2 = Date.now(); f.play(); console.log("PROBE play fallback", t2, f.lastBackend); }
            else if (step === 7) { p.source = ""; console.log("PROBE released"); }
            else if (step === 9) Qt.quit();
        }
    }
}
