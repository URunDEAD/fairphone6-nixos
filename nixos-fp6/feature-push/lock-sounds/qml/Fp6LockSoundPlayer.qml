/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * FP6 lock sounds: plays one short sound. WAV files (the bundled sounds and
 * the custom sounds fp6-lock-sounds prepared) go through SoundEffect: the
 * samples are decoded once when `source` is set and a PulseAudio stream
 * with media.role "event" (PipeWire: the event/system sounds role, under
 * the output volume) is ready, so play() starts within a few ms. Anything
 * else (a custom file not prepared yet, Opus, ...) falls back to a
 * QtMultimedia Audio player, created on first use.
 *
 * Used by Lomiri (qml/Components/Fp6LockSounds.qml) and by Settings > Sound
 * (preview). Setting `source` to "" releases the stream.
 */
import QtQuick 2.12
import QtMultimedia 5.6
import "Fp6LockSoundCatalog.js" as Catalog

Item {
    id: root

    property string source: ""          // a local path, or ""
    property string category: "event"   // PulseAudio media.role
    property real volume: 1.0

    readonly property bool isWav: /\.wav$/i.test(source)
    readonly property bool ready: isWav && effect.status === SoundEffect.Ready
    readonly property bool playing: effect.playing ||
        (fallback.item ? fallback.item.playbackState === Audio.PlayingState : false)
    // For tests: what the last play() used ("effect", "fallback" or "").
    property string lastBackend: ""

    signal played(string path, string backend)

    function play() {
        if (!source) { lastBackend = ""; return false; }
        stop();
        if (isWav && effect.status !== SoundEffect.Error && effect.status !== SoundEffect.Null) {
            effect.play(); // queued by SoundEffect while still Loading
            lastBackend = "effect";
        } else {
            if (isWav && effect.status === SoundEffect.Error) {
                // e.g. the prepared file appeared after the first load: retry next time
                effect.source = "";
                effect.source = Qt.binding(function() { return root.isWav ? Catalog.toUrl(root.source) : ""; });
            }
            fallback.active = true;
            fallback.item.source = Catalog.toUrl(source);
            fallback.item.play();
            lastBackend = "fallback";
        }
        played(source, lastBackend);
        return true;
    }

    function stop() {
        if (effect.playing) effect.stop();
        if (fallback.item) fallback.item.stop();
    }

    onSourceChanged: if (fallback.item && !source) fallback.active = false

    SoundEffect {
        id: effect
        source: root.isWav ? Catalog.toUrl(root.source) : ""
        category: root.category
        volume: root.volume
    }

    Loader {
        id: fallback
        active: false
        sourceComponent: Audio {
            audioRole: MediaPlayer.NotificationRole
            volume: root.volume
            onError: console.warn("Fp6LockSoundPlayer: " + errorString)
        }
    }
}
