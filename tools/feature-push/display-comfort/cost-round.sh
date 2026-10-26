#!/bin/sh
# Cost round on the installed build (filter part of Lomiri): no DM restart.
#   FP6_PW=... cost-round.sh DAEMON PLUGIN
L="sh /tmp/fp6-dc-in/live-test.sh"
$L attach "$2" "$1" || exit 1
$L keepon start
sh /tmp/fp6-dc-in/perf-steps.sh 2>&1
sleep 3; $L shot installed-night-off
$L settings 400,560
$L teardown 2>&1 | tail -2
pgrep -af "lomiri --mode=full" | cut -c1-60
loginctl list-sessions --no-legend | grep seat0
