#!/bin/sh
# Live check of a built fp6-notifyd in the phone's user session (no Lomiri
# patch needed): bus monitor on Notify, history off/on/body, ShouldAlert,
# indicator. Run as the phone user under the GUI lock (the test bubbles show):
#   flock -w 900 /run/user/10000/fp6-gui.lock sh daemon-live-test.sh /nix/store/...-fp6-notifyd-0.1.0
# Leaves nothing behind: closes its bubbles, resets its dconf keys, removes /tmp files.
set -u
PKG=$1
T=/tmp/fp6-notif-test
rm -rf "$T"; mkdir -p "$T/run" "$T/data" "$T/state"
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$(id -u)/bus
K=/com/fairphone6/notifications
before=$(dconf dump $K/)
XDG_DATA_HOME=$T/data XDG_STATE_HOME=$T/state FP6_GREYSCALE_KEY= \
  "$PKG/bin/fp6-notifyd" > "$T/log" 2>&1 &
pid=$!
sleep 2
ids=""
notify() { # app summary body
  id=$(gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications \
       -m org.freedesktop.Notifications.Notify "$1" 0 "" "$2" "$3" '[]' '{}' 3000 | tr -dc 0-9)
  ids="$ids $id"; sleep 1
}
status() { python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print("apps:", [a for a in d["apps"] if a.startswith("fp6-test")], "dnd:", d["dnd"], "entries:", [(e["app"], e["summary"], e.get("body")) for e in d["history"]["entries"] if e["app"].startswith("fp6-test")])' "/run/user/$(id -u)/fp6-notifyd/status.json"; }
echo "== 1 history off (default)"; notify fp6-test-a "Test one" "Body one"; status
echo "== 2 history on, no text"; dconf write $K/history-enabled true; sleep 1; notify fp6-test-a "Test two" "Body two"; status
echo "== 3 history with text"; dconf write $K/history-body true; sleep 1; notify fp6-test-b "Test three" "Body three"; status
ls -l "$T/data/fp6-notifyd/"
echo "== 4 history-body off strips text"; dconf write $K/history-body false; sleep 1; status
echo "== 5 DND / ShouldAlert"
D="busctl --user call com.fairphone6.Dnd /com/fairphone6/Dnd com.fairphone6.Dnd"
$D ShouldAlert ssbb call +999100000001 false false
$D SetActive b true; sleep 1
busctl --user get-property com.fairphone6.Dnd /com/fairphone6/Dnd com.fairphone6.Dnd Active Reason
echo "dconf dnd-active: $(dconf read $K/dnd-active) reason: $(dconf read $K/dnd-reason)"
$D ShouldAlert ssbb call +999100000002 false false
$D ShouldAlert ssbb call +999100000002 false false
$D ShouldAlert ssbb message +999100000003 true false
$D ShouldAlert ssbb message +999100000003 true true
busctl --user call com.lomiri.indicator.dnd /com/lomiri/indicator/dnd org.gtk.Actions Describe s dnd
$D SetActive b false; sleep 1
echo "dconf dnd-active after off: $(dconf read $K/dnd-active) manual: $(dconf read $K/dnd-manual)"
echo "== 6 history off deletes"; dconf write $K/history-enabled false; sleep 1; status; ls "$T/data/fp6-notifyd/" 2>&1
kill $pid; wait $pid 2>/dev/null
for i in $ids; do gdbus call --session -d org.freedesktop.Notifications -o /org/freedesktop/Notifications -m org.freedesktop.Notifications.CloseNotification "$i" >/dev/null 2>&1; done
echo "== log"; cat "$T/log"
dconf reset -f $K/
echo "dconf before: [$before] after: [$(dconf dump $K/)]"
rm -rf "$T" /run/user/$(id -u)/fp6-notifyd
