#!/bin/sh
# bring-up probe (root): 4 s of the voice path, records PCM states + kernel log.
# Only during a live call (fp6-csvoiced refuses otherwise: no-call opens crash
# the modem voice service). Extra args go to "fp6-csvoiced probe".
dmesg -C
systemctl stop fp6-csvoiced
( sleep 2; for f in /proc/asound/card0/pcm16p/sub0/status /proc/asound/card0/pcm17c/sub0/status; do echo "== $f"; cat $f; done ) &
fp6-csvoiced probe --seconds 4 "$@" 2>&1
wait
for c in "SEN_MI2S_RX Audio Mixer MultiMedia1" "SEN_MI2S_RX Audio Mixer VoiceMMode1" "VoiceMMode1 Mixer TX_CODEC_DMA_TX_3 Voice" "MultiMedia2 Mixer TX_CODEC_DMA_TX_3"; do printf '%s: ' "$c"; amixer -c0 cget name="$c" | tail -1; done
dmesg
systemctl start fp6-csvoiced
