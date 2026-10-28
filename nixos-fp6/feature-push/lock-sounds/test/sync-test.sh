#!/usr/bin/env bash
# fp6-lock-sounds sync with the real gsettings tool (keyfile backend in a
# temporary XDG_CONFIG_HOME, so the tool and this script share the values).
# Run inside fp6-ls-env (test/run-tests.sh does).
set -eu
w=$(mktemp -d); trap 'rm -rf "$w"' EXIT
export XDG_CONFIG_HOME=$w/config GSETTINGS_BACKEND=keyfile
GS() { "$FP6_GSETTINGS_BIN" "$@"; }
fail=0
check() { if eval "$2"; then echo "PASS $1"; else echo "FAIL $1"; fail=1; fi; }

mkdir -p "$w/Music"
src="$w/Music/It's a long tone.ogg"   # a long Ogg Vorbis file (lomiri-sounds' Ubuntu ringtone)
cp "$FP6_LS_RINGTONE" "$src"
check "test source present" '[ -s "$src" ]'

GS set com.fairphone6.lock-sounds lock-sound "\"$src\""
GS set com.fairphone6.lock-sounds unlock-sound "'chime'"
"$FP6_LOCK_SOUNDS" sync --cache "$w/cache" | tee "$w/out1"
check "custom file prepared" 'grep -q "prepared $src" "$w/out1"'
prepared=$(GS get com.fairphone6.lock-sounds prepared-files)
echo "prepared-files: $prepared"
wav=$(ls "$w"/cache/*.wav)
check "prepared-files lists [source, wav]" '[ "$(echo "$prepared" | grep -c "$wav")" = 1 ]'
info=$("$FP6_LOCK_SOUNDS" info "$wav"); echo "$info"
check "cut to 3 s" 'echo "$info" | grep -q " 3000 ms"'
check "peak -9 dBFS" 'echo "$info" | grep -q "peak -9.0 dBFS"'

"$FP6_LOCK_SOUNDS" sync --cache "$w/cache" > "$w/out2"
check "second run does nothing" '[ ! -s "$w/out2" ]'

printf 'not audio' > "$w/Music/broken.mp3"
GS set com.fairphone6.lock-sounds unlock-sound "'$w/Music/broken.mp3'"
"$FP6_LOCK_SOUNDS" sync --cache "$w/cache" > "$w/out3" 2> "$w/err3"; rc=$?
cat "$w/err3"
check "broken file: reported, exit 0" '[ $rc = 0 ] && grep -q "could not prepare" "$w/err3"'

GS set com.fairphone6.lock-sounds lock-sound "'pop-down'"
GS set com.fairphone6.lock-sounds unlock-sound "'pop-up'"
"$FP6_LOCK_SOUNDS" sync --cache "$w/cache" | tee "$w/out4"
check "back to bundled: cache emptied" '[ -z "$(ls "$w/cache")" ]'
check "back to bundled: prepared-files empty" '[ "$(GS get com.fairphone6.lock-sounds prepared-files)" = "@as []" ]'
exit $fail
