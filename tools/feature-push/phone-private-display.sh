#!/usr/bin/env bash
# Private headless Wayland display on the FP6 for one agent, so test apps do not
# open on (or take input from) the user's Lomiri session.
#
# Usage (on the laptop):
#   tools/feature-push/phone-private-display.sh start NAME   # prints the env file path + contents
#   tools/feature-push/phone-private-display.sh stop NAME
#
# Inside the private display (on the phone, as user `user`):
#   . /run/user/10000/fp6-private-NAME.env; some-app &
#   grim /tmp/shot.png                                   # screenshot (wlr-screencopy)
#   wlrctl pointer move X Y; wlrctl pointer click left   # tap (virtual pointer; X/Y relative moves)
#   wtype 'text'; wtype -k Return                        # type (virtual keyboard)
# Output is 1116x2412 at scale 3, like the phone panel (372x804 logical).
# Maliit (OSK) and Lomiri-specific behaviour are NOT available here; test those
# in the real session only after asking the coordinator.
set -euo pipefail
cmd=${1:?start|stop}; name=${2:?name}
here=$(cd "$(dirname "$0")/../.." && pwd)
"$here/tools/phone-ssh" 'mkdir -p ~/.local/bin && cat > ~/.local/bin/fp6-private-display && chmod +x ~/.local/bin/fp6-private-display' <<'PHONE'
#!/usr/bin/env bash
set -euo pipefail
cmd=$1; name=$2
SWAY=/nix/store/d9x7bh5gdzdfhq67fs34dfv8smdlgn9z-sway-1.12/bin
TOOLS=/nix/store/ygphy96xnym1z1da7a9cy24c1lhqmqxb-wlrctl-0.2.2/bin:/nix/store/59cr57wv8lbcdag3dd1297zky8j3np65-wtype-0.4/bin:/nix/store/lirz5j9fyxvzhzbl2ksgvbmpnj8jpshf-grim-1.5.0/bin
rt=/run/user/$(id -u)
unit=fp6-private-$name
envf=$rt/fp6-private-$name.env
socks() { (cd "$rt" && ls -1 wayland-[0-9] wayland-[0-9][0-9] 2>/dev/null | sort) || true; }
case $cmd in
start)
  systemctl --user stop "$unit" 2>/dev/null || true
  before=$(socks)
  ipc_before=$(ls -1 "$rt"/sway-ipc.*.sock 2>/dev/null | sort || true)
  : > "$rt/$unit.sway"
  systemd-run --user --unit="$unit" --collect \
    -E WLR_BACKENDS=headless -E WLR_LIBINPUT_NO_DEVICES=1 -E WLR_RENDERER=pixman \
    -E WAYLAND_DISPLAY= -E XDG_RUNTIME_DIR="$rt" \
    "$SWAY/sway" -c "$rt/$unit.sway" >/dev/null 2>&1
  sock=; ipc=
  for _ in $(seq 1 50); do
    sock=$(comm -13 <(printf '%s\n' "$before") <(socks) | head -1)
    ipc=$(comm -13 <(printf '%s\n' "$ipc_before") <(ls -1 "$rt"/sway-ipc.*.sock 2>/dev/null | sort) | head -1)
    [ -n "$sock" ] && [ -S "$rt/$sock" ] && [ -n "$ipc" ] && break
    sleep 0.2
  done
  if [ -z "$sock" ] || [ -z "$ipc" ]; then
    journalctl --user -u "$unit" -o cat --no-pager | tail -20; exit 1
  fi
  "$SWAY/swaymsg" -s "$ipc" output HEADLESS-1 resolution 1116x2412 scale 3 >/dev/null
  cat > "$envf" <<EOF
export WAYLAND_DISPLAY=$sock XDG_RUNTIME_DIR=$rt SWAYSOCK=$ipc
export QT_QPA_PLATFORM=wayland GDK_BACKEND=wayland
export PATH=$TOOLS:\$PATH
unset DISPLAY
EOF
  echo "$envf"; cat "$envf"
  ;;
stop)
  systemctl --user stop "$unit" 2>/dev/null || true
  rm -f "$envf" "$rt/$unit.sway"
  ;;
esac
PHONE
exec "$here/tools/phone-ssh" "~/.local/bin/fp6-private-display $cmd $name"
