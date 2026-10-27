#!/bin/sh
# usage: swap-tplg.sh <dir containing qcom/milos/Fairphone (Gen. 6)-tplg.bin> | orig
# Bring-up only (root): stops PipeWire (user + greeter) and fp6-csvoiced, points
# the firmware loader at an overlay of the system firmware with that topology,
# rebinds the sound card (topology reload), restores the loader path and
# restarts the services. Used live 2026-10-27 (README "Live bring-up").
set -u
V=$1
ORIG=$(cat /run/fp6-fw-orig 2>/dev/null || cat /sys/module/firmware_class/parameters/path)
echo "$ORIG" > /run/fp6-fw-orig
TPLG='qcom/milos/Fairphone (Gen. 6)-tplg.bin'
if [ "$V" = orig ]; then NEW=$ORIG; else
  NEW=/run/fp6-fwov; rm -rf $NEW; mkdir -p $NEW/qcom/milos
  for e in "$ORIG"/*; do [ "$(basename "$e")" = qcom ] || ln -s "$e" $NEW/; done
  for e in "$ORIG"/qcom/*; do [ "$(basename "$e")" = milos ] || ln -s "$e" $NEW/qcom/; done
  for e in "$ORIG"/qcom/milos/*; do [ "$(basename "$e")" = "Fairphone (Gen. 6)-tplg.bin" ] || ln -s "$e" $NEW/qcom/milos/; done
  cp "$V/$TPLG" "$NEW/$TPLG"
fi
for u in 10000 78; do
  systemctl --machine=$(id -nu $u)@.host --user stop pipewire.socket pipewire-pulse.socket wireplumber pipewire-pulse pipewire 2>/dev/null
done
systemctl stop fp6-csvoiced
sleep 1
printf '%s' "$NEW" > /sys/module/firmware_class/parameters/path
echo sound > /sys/bus/platform/drivers/snd-sc8280xp/unbind
sleep 1
echo sound > /sys/bus/platform/drivers/snd-sc8280xp/bind
sleep 3
printf '%s' "$ORIG" > /sys/module/firmware_class/parameters/path
cat /proc/asound/pcm
sha256sum "$NEW/$TPLG" | cut -c1-16
for u in 10000 78; do
  systemctl --machine=$(id -nu $u)@.host --user start pipewire.socket pipewire-pulse.socket wireplumber 2>/dev/null
done
systemctl start fp6-csvoiced
