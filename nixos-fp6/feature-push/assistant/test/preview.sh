#!/usr/bin/env bash
# Offscreen previews of the Assistant UI with canned, synthetic data.
#   test/preview.sh ENV_DIR UI_BINARY OUTDIR [scene ...]
# Exit status = number of failed scenes (PASS/FAIL lines are printed).
set -u
env_dir=$1; bin=$2; out=$3; shift 3
here=$(cd "$(dirname "$0")/.." && pwd)
scenes=("$@")
[ ${#scenes[@]} -eq 0 ] && scenes=(home springboard chat cloud permission permission-tainted confirm activity settings backend tools screenshot)
mkdir -p "$out"
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/runtime"; chmod 700 "$work/runtime"
fails=0
for s in "${scenes[@]}"; do
  log="$work/$s.log"
  if ! XDG_RUNTIME_DIR="$work/runtime" HOME="$work" QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=16 \
       FP6_ASSISTANT_QML_DIR="$here/qml" LIBGL_ALWAYS_SOFTWARE=1 \
       timeout 60 "$env_dir/bin/fp6-assistant-env" "$bin" --preview "$s" --screenshot "$out/$s.png" >"$log" 2>&1; then
    echo "FAIL $s: exit status"; fails=$((fails+1))
  fi
  grep -E "PASS|FAIL|DONE" "$log"
  # QML errors in our files fail the scene.
  if grep -E "assistant/qml/[A-Za-z]+\.qml:[0-9]+" "$log"; then echo "FAIL $s: QML warnings/errors"; fails=$((fails+1)); fi
  if [ ! -s "$out/$s.png" ]; then echo "FAIL $s: no screenshot"; fails=$((fails+1)); fi
done
exit $fails
