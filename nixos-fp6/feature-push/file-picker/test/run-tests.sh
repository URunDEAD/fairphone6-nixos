#!/usr/bin/env bash
# Laptop tests of the portal chain (xdg-desktop-portal 1.20 frontend ->
# xdg-desktop-portal-lomiri -> fp6-file-picker under Xvfb, scale 3,
# 372x804 logical like the phone). Writes screenshots and a log to OUT.
#   test/run-tests.sh PKG XDP OUT
# Needs on PATH: xvfb-run, dbus-run-session, python3 with gi (Gio, Gtk 4).
set -u
pkg=$1 xdp=$2 out=$3
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
home=$(mktemp -d)/home
bash "$here/fake-home.sh" "$home"
export FAKE_HOME=$home FP6_PICKER_QML=${FP6_PICKER_QML:-$pkg/share/fp6-file-picker/qml}
session() { timeout 120 xvfb-run -a -s '-screen 0 1200x2600x24' bash "$here/portal-session.sh" "$pkg" "$xdp" "$@"; }
client() { session python3 "$here/portal-client.py" "$@"; }
pv() { local p=$1; shift; FP6_PREVIEW=$p client "$@"; }
# GTK 4 GtkFileDialog (decides on its own to use the portal), X11 under Xvfb.
gtk4() { local p=$1; shift; [ -z "${GTK4_SCHEMAS:-}" ] || export GSETTINGS_SCHEMA_DIR=$GTK4_SCHEMAS; FP6_PREVIEW=$p GDK_BACKEND=x11 GTK_A11Y=none session python3 "$here/gtk4-open.py" "$@"; }
pass=0 fail=0
check() { # name expected-substring command...
  local name=$1 want=$2; shift 2
  local got
  got=$("$@" 2>"$out/$name.stderr" | tail -1)
  if [[ "$got" == *"$want"* ]]; then pass=$((pass+1)); echo "PASS $name: $got"
  else fail=$((fail+1)); echo "FAIL $name: got '$got' want '*$want*'"; fi
}
shot() { # name preview client-args...
  local name=$1 prev=$2; shift 2
  FP6_GRAB=$out/$name.png:${GRAB_DELAY:-3500} FP6_GRAB_SIZE=372x804 FP6_PREVIEW=$prev client "$@" --timeout 40 >/dev/null 2>"$out/$name.stderr"
  [ -s "$out/$name.png" ] && echo "SHOT $name" || { echo "FAIL shot $name"; fail=$((fail+1)); }
}

# Screenshots
shot 01-open-torrent-recent "" open --torrent --title "Open Torrent"
shot 02-open-torrent-downloads "place=downloads" open --torrent --title "Open Torrent"
shot 03-filter-menu "place=downloads,filters" open --torrent --title "Open Torrent"
shot 04-open-multiple "place=downloads,select2" open --multiple --title "Upload files"
shot 05-save-file "place=documents" save --name "report-final.pdf" --title "Save PDF"
shot 06-save-replace "place=documents,name=budget.ods,replace" save --title "Save Spreadsheet"
shot 07-new-folder "place=documents,newfolder" save --name "photo.jpg"
shot 08-select-folder "place=home" folder --title "Download folder"
shot 09-choices "place=documents" open --choices --title "Open Text"
UITK_THEME=Lomiri.Components.Themes.SuruDark shot 10-dark-recent "" open --torrent --title "Open Torrent"
shot 11-other-apps "apps" open --title "Upload photo"
FP6_GRAB=$out/12-gtk4-open-torrent.png:4000 FP6_GRAB_SIZE=372x804 gtk4 "" >/dev/null 2>"$out/12-gtk4-open-torrent.stderr"
[ -s "$out/12-gtk4-open-torrent.png" ] && echo "SHOT 12-gtk4-open-torrent" || { echo "FAIL shot 12-gtk4-open-torrent"; fail=$((fail+1)); }

# Behaviour (answers through the real frontend)
check open-pick 'file://'"$home"'/Downloads/nixos-minimal-26.05-aarch64.iso.torrent' \
  pv "place=downloads,pick=$home/Downloads/nixos-minimal-26.05-aarch64.iso.torrent" open --torrent
check open-filter-result '"current_filter": ["Torrent files"' \
  pv "place=downloads,pick=$home/Downloads/nixos-minimal-26.05-aarch64.iso.torrent" open --torrent
check open-multiple '.torrent", "file://' \
  pv "place=downloads,select2,accept" open --multiple
check open-current-folder "$home/Pictures/screenshot-lomiri.png" \
  pv "pick=$home/Pictures/screenshot-lomiri.png" open --folder "$home/Pictures"
check save-new "file://$home/Documents/new%20report.pdf" \
  pv "place=documents,accept" save --name "new report.pdf"
check save-files-unique "file://$home/Downloads/notes%20(2).txt" \
  pv "place=downloads,accept" save-files
check select-folder "file://$home/Music\"" \
  pv "place=music,accept" folder
check choices '["encoding", "utf8"]' \
  pv "place=documents,pick=$home/Documents/report.odt" open --choices
check cancel '"response": 1' pv "cancel" open
check request-close '"closed": true' client open --close-after 3
check gtk4-open "PICKED $home/Downloads/debian-13.2.0-arm64-netinst.iso.torrent" \
  gtk4 "place=downloads,pick=$home/Downloads/debian-13.2.0-arm64-netinst.iso.torrent"
check gtk4-save "PICKED $home/Documents/new.torrent" gtk4 "place=documents,accept" save
check gtk4-cancel "ERROR" gtk4 "cancel"
echo "passed=$pass failed=$fail"
[ "$fail" = 0 ]
