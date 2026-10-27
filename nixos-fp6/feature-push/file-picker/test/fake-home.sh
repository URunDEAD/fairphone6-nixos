#!/usr/bin/env bash
# Synthetic home directory for the offline picker tests (no personal data).
#   test/fake-home.sh DIR
set -eu
h=$1
mkdir -p "$h"/{Documents/Notes,Downloads,Music/Album,Pictures/Camera,Videos,.config,.local/share}
touch -d '2026-10-27 09:12' "$h/Downloads/debian-13.2.0-arm64-netinst.iso.torrent"
touch -d '2026-10-26 18:40' "$h/Downloads/nixos-minimal-26.05-aarch64.iso.torrent"
touch -d '2026-10-20 11:03' "$h/Downloads/archlinux-2026.10.01-x86_64.iso.torrent"
for n in invoice-2026-10.pdf map-export.gpx fairphone6-manual.pdf; do : > "$h/Downloads/$n"; done
head -c 734003 /dev/zero > "$h/Downloads/fairphone6-manual.pdf"
touch -d '2026-10-25 08:00' "$h/Downloads/invoice-2026-10.pdf" "$h/Downloads/map-export.gpx"
touch -d '2026-10-24 21:15' "$h/Downloads/fairphone6-manual.pdf"
echo x > "$h/Downloads/notes.txt"
for n in shopping.txt ideas.md; do echo x > "$h/Documents/Notes/$n"; done
: > "$h/Documents/report.odt"; : > "$h/Documents/budget.ods"
head -c 2400000 /dev/zero > "$h/Pictures/Camera/IMG_20261026_101500.jpg"
head -c 2100000 /dev/zero > "$h/Pictures/Camera/IMG_20261026_101512.jpg"
touch -d '2026-10-26 10:15' "$h/Pictures/Camera/IMG_20261026_101500.jpg"
touch -d '2026-10-26 10:15' "$h/Pictures/Camera/IMG_20261026_101512.jpg"
: > "$h/Pictures/screenshot-lomiri.png"
: > "$h/Music/Album/01-track.flac"
head -c 31000000 /dev/zero > "$h/Videos/VID_20261025_170000.mp4"
: > "$h/.hidden-file"
cat > "$h/.config/user-dirs.dirs" <<'EOF'
XDG_DESKTOP_DIR="$HOME/"
XDG_DOCUMENTS_DIR="$HOME/Documents"
XDG_DOWNLOAD_DIR="$HOME/Downloads"
XDG_MUSIC_DIR="$HOME/Music"
XDG_PICTURES_DIR="$HOME/Pictures"
XDG_VIDEOS_DIR="$HOME/Videos"
EOF
