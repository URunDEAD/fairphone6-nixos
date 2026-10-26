#!/bin/sh
# Measurement steps of the display-comfort live test (on the phone, after
# live-test.sh setup, before teardown). FP6_PW in the environment.
L="sh /tmp/fp6-dc-in/live-test.sh"
set -x
$L shot base
$L gs greyscale-active true; sleep 2.5; $L shot grey; $L gs greyscale-active false; sleep 2
$L gs night-light-schedule always; $L gs night-light-temperature 3400; $L gs night-light-enabled true
sleep 4; $L shot night3400
$L get | grep -E "current|status"
journalctl --user -b --since "-10 min" -o cat | grep -E "FP6 ColorFilter|ShellScreen|DisplayComfort|fp6-display-comfort" | tail -10
$L load start; sleep 3
echo "== load, night light 3400 K (blend)"; $L sample 20
$L shot night-load
$L gs night-light-enabled false; sleep 3
echo "== load, filter off (no node)"; $L sample 20
$L gs greyscale-active true; sleep 2
echo "== load, greyscale"; $L sample 20
$L shot grey-load
$L gs greyscale-active false; sleep 2
$L load stop
echo "== idle, filter off"; $L sample 10
journalctl --user -b --since "-10 min" -o cat | grep -E "FP6 ColorFilter" | tail -5
$L gs dark-theme-mode on; sleep 2
cat ~/.config/lomiri-ui-toolkit/theme.ini; dconf read /org/gnome/desktop/interface/color-scheme
$L shot dark-on
$L gs dark-theme-mode off
