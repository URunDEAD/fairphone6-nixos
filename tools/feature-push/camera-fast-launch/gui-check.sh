#!/usr/bin/env bash
# Settings page with the app-ready schema, then secure mode (phone side).
#   gui-check.sh OUTDIR APP_BIN SCHEMA_DIR   (every step only after the previous one succeeded)
set -u
out=$1 app=$2 schemas=$3
g=/tmp/fl/gui.sh
mkdir -p "$out"
step() { "$@" || { echo "FAILED: $*"; $g stop; exit 1; }; }
step $g start "$app" GSETTINGS_SCHEMA_DIR=$schemas
sleep 2
step $g touch 1000 1100 150 1100 250; sleep 2      # swipe to the photo roll
step $g touch 1060 157; sleep 1.5                  # overflow menu
step $g touch 860 743; sleep 2                     # Settings
$g shot > "$out/settings-schema.png" || exit 1
step $g touch 300 1150; sleep 1.5                  # "Preload" row (checked by screenshot)
$g shot > "$out/settings-preload.png" || exit 1
gsettings_out=$(GSETTINGS_SCHEMA_DIR=$schemas gsettings get com.fairphone6.app-ready preload 2>&1)
echo "preload list: $gsettings_out" > "$out/gsettings.txt"
step $g touch 300 1050; sleep 1.5                  # back to "Off"
GSETTINGS_SCHEMA_DIR=$schemas gsettings get com.fairphone6.app-ready preload >> "$out/gsettings.txt" 2>&1
$g shot > "$out/settings-off.png"
$g stop
# secure mode: the photo roll must be empty (no earlier photos)
step $g start "$app" FP6_CAMERA_SECURE=1
sleep 2
step $g touch 1000 1100 150 1100 250; sleep 2
$g shot > "$out/secure-roll.png"
step $g touch 1060 157; sleep 1.5
$g shot > "$out/secure-menu.png"
$g stop
echo "CHECK DONE"
