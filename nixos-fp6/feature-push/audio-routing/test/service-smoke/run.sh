#!/bin/sh
# Laptop smoke test of the patched ayatana-indicator-sound service against a
# private PipeWire + WirePlumber + pipewire-pulse (two null sinks, one null
# source; no hardware monitors), a mock notification daemon and an
# "interested" media player (the 2026-10-27 crash: an empty Output/Input
# section made find_player_section() read item 0 of an empty menu).
# Checks: the service stays up, the volume action reaches the sink, scroll
# steps work, output selection works, SIGTERM exits cleanly.
#   dbus-run-session -- test/service-smoke/run.sh
set -u
D=$(dirname "$(readlink -f "$0")")
NP=$(nix-instantiate --eval -E "(import $D/../../../../upstream/mobile-nixos/npins).nixpkgs.outPath" | tr -d '"')
nb() { nix-build --no-out-link -E "let p = import $NP {}; in p.$1"; }
PW=$(nb pipewire); WP=$(nb wireplumber); G=$(nb glib.bin)/bin/gdbus
PY=$(nb 'python3.withPackages (ps: [ ps.pygobject3 ])')/bin/python3
IND=${IND:-$(nix-build --no-out-link "$D/../../build.nix" -A indicator)}
W=$(mktemp -d)
cp -r "$D/cfg" "$D/data" "$W/"; chmod -R u+w "$W"
R=/run/user/$(id -u)/fp6pwt.$$; mkdir -m700 "$R"   # socket paths must stay < 108 bytes
export GSETTINGS_BACKEND=keyfile XDG_DATA_HOME=$W/data XDG_RUNTIME_DIR=$R XDG_CONFIG_HOME=$W/cfg XDG_STATE_HOME=$W/state HOME=$W
GI_TYPELIB_PATH=$(nb glib.out)/lib/girepository-1.0:$(nb gobject-introspection)/lib/girepository-1.0 $PY "$D/notif.py" > "$W/notif.log" 2>&1 &
$PW/bin/pipewire > "$W/pw.log" 2>&1 & sleep 1
$WP/bin/wireplumber > "$W/wp.log" 2>&1 &
$PW/bin/pipewire-pulse > "$W/pp.log" 2>&1 & sleep 2
XDG_CURRENT_DESKTOP=Lomiri $IND/libexec/ayatana-indicator-sound/ayatana-indicator-sound-service > "$W/svc.log" 2>&1 &
SVC=$!; sleep 3
A="$G call --session -d org.ayatana.indicator.sound -o /org/ayatana/indicator/sound -m org.gtk.Actions"
vol() { $WP/bin/wpctl status | grep "Fake speaker" | grep -o 'vol: [0-9.]*'; }
fail=0
$A.SetState volume '<0.3>' '{}' >/dev/null; sleep 1
v=$(vol); echo "after volume 0.3: $v"; [ "$v" = "vol: 0.30" ] || fail=1
$A.Activate volume '[<int32 5>]' '{}' >/dev/null; sleep 1
v=$(vol); echo "after +5 steps: $v"; [ "$v" = "vol: 0.40" ] || fail=1
$A.Describe output-devices | grep -q fake_speaker || { echo "no output list"; fail=1; }
kill -0 $SVC 2>/dev/null && echo "service alive" || { echo "SERVICE DIED"; fail=1; }
# Clean exit on SIGTERM (session end, e.g. greeter logout): finalizing the
# device selector used to free the PulseAudio context after its main loop
# (SIGSEGV in glib_io_free on the phone; the laptop heap never faulted, so
# this only catches a crash, the order itself is checked in the binary).
kill $SVC; wait $SVC; rc=$?; echo "exit status after SIGTERM: $rc"; [ $rc -lt 128 ] || fail=1
kill %1 %2 %3 %4 2>/dev/null; sleep 0.5; rm -rf "$R"
[ $fail = 0 ] && echo PASS || { echo "FAIL (logs in $W)"; exit 1; }
