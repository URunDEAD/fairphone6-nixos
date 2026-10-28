# fp6-keyboard-render OUTDIR
# Renders the real (patched) maliit-keyboard under Xvfb, driven through the
# Maliit D-Bus input context by a QML text field (render-client.qml), and
# saves screenshots into OUTDIR. Taps go through the on-screen keys (xdotool
# on the keyboard window), so the word ribbon shows real predictions.
# Fails if the keyboard log has QML errors (layout load check).
# Environment (set by render.nix / test/default.nix): MALIIT_KEYBOARD,
# MALIIT_FRAMEWORK, QT_BIN_PLUGINS, QT_QML, QMLSCENE, CLIENT_QML.
set -euo pipefail
out=${1:?usage: fp6-keyboard-render OUTDIR}
mkdir -p "$out"
work=$(mktemp -d)
cleanup() {
  kill "${pids[@]}" 2>/dev/null || true
  cp "$work"/*.log "$out"/ 2>/dev/null || true
  rm -rf "$work"
}
pids=()
trap cleanup EXIT

W=${FP6_RENDER_W:-1080}
H=${FP6_RENDER_H:-2340}
export HOME=$work/home XDG_CONFIG_HOME=$work/config XDG_DATA_HOME=$work/data XDG_RUNTIME_DIR=$work/run
mkdir -p "$HOME" "$XDG_CONFIG_HOME/glib-2.0/settings" "$XDG_DATA_HOME" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
export GSETTINGS_BACKEND=keyfile GRID_UNIT_PX=24 QT_QUICK_BACKEND=software QT_QUICK_CONTROLS_STYLE=Default
export LANG=C.UTF-8 QT_QPA_PLATFORM=xcb
# Nothing from the host's Qt/GTK environment.
unset QT_PLUGIN_PATH QML2_IMPORT_PATH QML_IMPORT_PATH NIXPKGS_QT5_QML_IMPORT_PATH QT_IM_MODULE \
  GTK_IM_MODULE XMODIFIERS WAYLAND_DISPLAY GIO_EXTRA_MODULES XDG_DATA_DIRS 2>/dev/null || true

Xvfb :87 -screen 0 "${W}x${H}x24" -nolisten tcp >"$work/xvfb.log" 2>&1 &
pids+=($!)
export DISPLAY=:87
for _ in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
# maliit draws into a translucent (ARGB) window: needs a compositing manager.
xcompmgr >"$work/xcompmgr.log" 2>&1 &
pids+=($!)

# Private session bus without service activation (nothing from the host).
cat >"$work/session.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$work/bus</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
dbus-daemon --config-file="$work/session.conf" --fork --print-pid >"$work/dbus.pid"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$work/bus"
pids+=("$(cat "$work/dbus.pid")")

# Suru icons (Lomiri's theme) for shift/backspace/enter: Qt's generic theme
# looks up "hicolor", so a user-level hicolor index inherits suru.
mkdir -p "$XDG_DATA_HOME/icons/hicolor"
printf '[Icon Theme]\nName=hicolor\nInherits=suru\nDirectories=\n' >"$XDG_DATA_HOME/icons/hicolor/index.theme"
export XDG_DATA_DIRS="$XDG_DATA_HOME:$SURU_ICONS/share"

kbd_pid=
start_keyboard() { # active-language enabled-languages [extra keyfile lines]
  [ -n "$kbd_pid" ] && { kill "$kbd_pid" 2>/dev/null || true; wait "$kbd_pid" 2>/dev/null || true; }
  cat >"$XDG_CONFIG_HOME/glib-2.0/settings/keyfile" <<KEYS
[org/maliit/keyboard/maliit]
active-language='$1'
enabled-languages=$2
${3:-}
KEYS
  "$MALIIT_KEYBOARD/bin/maliit-keyboard" >>"$work/maliit.log" 2>&1 &
  kbd_pid=$!
  pids+=("$kbd_pid")
  sleep 4
}

start_client() {
  QT_IM_MODULE=maliit QT_PLUGIN_PATH="$MALIIT_FRAMEWORK:$QT_BIN_PLUGINS" QML2_IMPORT_PATH="$QT_QML" \
    "$QMLSCENE" "$CLIENT_QML" >>"$work/client.log" 2>&1 &
  client_pid=$!
  pids+=("$client_pid")
  sleep 5
}
# Clear the field between scenarios via the client's clear button (resets
# the input method, so no preedit is re-committed).
fresh_client() { tap $((W - 120)) 60; sleep 0.8; }

# Bottom part of the screen: text field is not visible there, so a strip of
# the field is appended on top for the ribbon shots.
shot() {
  import -window root "$work/full.png"
  convert "$work/full.png" \( -clone 0 -crop 1080x150+0+100 \) \( -clone 0 -crop 1080x830+0+1510 \) -delete 0 -append "$out/$1.png"
  echo "saved $out/$1.png"
}

# Keyboard geometry (pixels) for W x H with GRID_UNIT_PX=24, measured from
# the first screenshot (1080x2340): first key row top, extra offset, row pitch.
KB_TOP=${FP6_KB_TOP:-1638}
RIBBON=${FP6_KB_RIBBON:-0}
ROW=${FP6_KB_ROW:-175}
KW=$((W / 10))
key() { # row index-in-row keys-in-row -> "x y"
  local x0=$(( (W - $3 * KW) / 2 ))
  echo "$(( x0 + $2 * KW + KW / 2 )) $(( KB_TOP + RIBBON + $1 * ROW + ROW / 2 ))"
}
tap() { xdotool mousemove "$1" "$2" click 1; sleep 0.35; }
hold() { xdotool mousemove "$1" "$2" mousedown 1; sleep 1.5; }
release() { xdotool mouseup 1; sleep 0.5; }

# Letters of the qwerty rows: row 0 qwertyuiop (10), row 1 asdfghjkl (9),
# row 2 zxcvbnm (7, between shift and backspace).
declare -A POS
r0=qwertyuiop; r1=asdfghjkl; r2=zxcvbnm
for i in $(seq 0 9); do POS[${r0:$i:1}]="0 $i 10"; done
for i in $(seq 0 8); do POS[${r1:$i:1}]="1 $i 9"; done
for i in $(seq 0 6); do POS[${r2:$i:1}]="2 $i 7"; done
type_osk() { # lowercase ascii letters and spaces
  local s=$1 c
  for ((i = 0; i < ${#s}; i++)); do
    c=${s:$i:1}
    if [ "$c" = " " ]; then tap $(space_xy); else tap $(key ${POS[$c]}); fi
  done
}
space_xy() { echo "$(( W / 2 )) $(( KB_TOP + RIBBON + 3 * ROW + ROW / 2 ))"; }

start_client
start_keyboard ro "['en', 'ro']"
sleep 2
if [ -n "${FP6_RENDER_DEBUG:-}" ]; then
  xwininfo -root -tree >"$out/windows.txt" 2>&1 || true
  kid=$(xwininfo -root -tree | awk '/"maliit-keyboard": \("maliit-keyboard"/ {print $1}')
  import -window "$kid" "$out/debug-kbd-window.png" || true
  xwd -root -silent | convert xwd:- "$out/debug-xwd.png" || true
fi
shot ro-keyboard


# Long-press popups on the Romanian layout.
for k in a s t i; do
  hold $(key ${POS[$k]}); shot "ro-longpress-$k"; release
done
fresh_client

# Word ribbon: diacritic-free typing gets the Romanian word offered.
type_osk "multumesc fr"; shot ro-ribbon-multumesc-fr
fresh_client
type_osk "si"; shot ro-ribbon-si
fresh_client
type_osk "ne vedem ma"; shot ro-ribbon-ne-vedem-ma
fresh_client

# Space swipe right -> next language (en).
read -r sx sy < <(space_xy)
xdotool mousemove $((sx - 200)) "$sy" mousedown 1 mousemove $((sx - 100)) "$sy" mousemove "$sx" "$sy" \
  mousemove $((sx + 150)) "$sy" mouseup 1
sleep 1.5
shot lang-switch-space-swipe-to-en
type_osk "see you to"; shot en-ribbon-see-you-to
fresh_client

# Dictation key (maliit-keyboard-dictation-key.patch) against the fake
# com.fairphone6.Dictation1 (tests/fp6/fake_dictation_service.h): no
# microphone, Stop returns a fixed synthetic sentence after 1.5 s.
# Setting on but no service: no key (the bottom row is unchanged).
"$FAKE_DICTATION" "Ne vedem mâine la cafea." >"$work/fake.log" 2>&1 &
fake_pid=$!
pids+=("$fake_pid")
sleep 1
grep -q ready "$work/fake.log"
start_keyboard ro "['en', 'ro']" "dictation-key=true"
fresh_client
type_osk "salut"
mic_x=$(( KW + 48 + KW + KW / 2 )) # symbol key (+ action padding 2 gu), globe, mic
read -r _ sy < <(space_xy)
tap "$mic_x" "$sy"; sleep 0.8
shot dictation-listening
tap "$mic_x" "$sy"; sleep 0.5
shot dictation-transcribing
sleep 2.5
shot dictation-inserted
grep -q 'fp6-field-text: \[Salut Ne vedem mâine la cafea. \]' "$work/client.log" \
  || { echo "dictation text not inserted:" >&2; grep fp6-field-text "$work/client.log" | tail -3 >&2; exit 1; }
# Long-press while listening cancels: nothing more is inserted.
tap "$mic_x" "$sy"; sleep 0.8
hold "$mic_x" "$sy"; release
sleep 2.5
shot dictation-cancelled
last=$(grep fp6-field-text "$work/client.log" | tail -1)
[ "$last" = 'qml: fp6-field-text: [Salut Ne vedem mâine la cafea. ]' ] \
  || { echo "text changed after cancel: $last" >&2; exit 1; }
kill "$fake_pid"; sleep 1
shot dictation-service-gone
fresh_client

cp "$work/maliit.log" "$out/maliit.log"
cp "$work/client.log" "$out/client.log"
if grep -E 'qml:.*(Error|error)|is not a type|ReferenceError|TypeError|Cannot assign|file:.*\.qml:[0-9]+' "$work/maliit.log" \
    | grep -v 'Binding loop' >"$out/qml-errors.txt"; then
  echo "QML errors in the keyboard log:" >&2; cat "$out/qml-errors.txt" >&2; exit 1
fi
rm -f "$out/qml-errors.txt"
grep -h 'fp6 ngram\|Did not find a dictionary\|No dictionary\|aff_file' "$work/maliit.log" | sort -u >"$out/dictionaries-and-models.txt" || true
echo "render ok"
