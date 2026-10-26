#!/bin/sh
# Cost of the colour filter in the running shell (after live-test.sh attach):
# a full-screen animated client keeps the shell compositing every frame.
L="sh /tmp/fp6-dc-in/live-test.sh"
$L gs night-light-schedule always; $L gs night-light-temperature 3400
$L load start; sleep 6
for round in 1 2; do
  echo "== [$round] filter off (no node)"; $L gs night-light-enabled false; $L gs greyscale-active false; sleep 3; $L sample 15
  echo "== [$round] night light 3400 K (blend)"; $L gs night-light-enabled true; sleep 4; $L sample 15
  echo "== [$round] night light + greyscale (fetch)"; $L gs greyscale-active true; sleep 3; $L sample 15
done
$L gs night-light-enabled false; $L gs greyscale-active false; sleep 2
$L load stop
echo "== idle, filter off, no load"; $L sample 10
$L gs night-light-enabled true; sleep 4
echo "== idle, night light on, no load"; $L sample 10
$L gs night-light-enabled false
journalctl --user -b --since "-5 min" -o cat | grep -E "FP6 ColorFilter" | tail -3
