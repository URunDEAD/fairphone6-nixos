#!/usr/bin/env bash
# FP6 sound live test. Run ON THE PHONE as the logged-in session user (the one
# owning the PipeWire session), after booting/activating a generation that
# contains nixos-fp6/feature-push/audio/audio-fp6-feature.nix:
#   tools/phone-ssh "cat > /tmp/fp6-audio-test.sh" < tools/feature-push/audio-test.sh
#   tools/phone-ssh "bash /tmp/fp6-audio-test.sh precheck"
# Steps: precheck | speaker | earpiece | mic | mic2 | volume | keys | usb |
#        callroute | all   ("all" = every non-interactive step, in order)
#
# Safety:
#  * Every playback first verifies both AW88261 amp volumes are <= 352
#    (-2 dB, pmOS-tested) and aborts otherwise; the script never raises amp
#    volume itself (only UCM does, capped at 352 / 320 earpiece).
#  * Tones are 440 Hz at -20 dBFS (speaker) / -30 dBFS (earpiece), 2 s, with
#    the sink volume temporarily set to 0.50 (0.30 earpiece) and restored.
#  * Earpiece test: hold the phone ~10 cm from the ear, never against it.
#  * No call is placed; callroute only switches the card profile/route and
#    reads mixer values back, then restores HiFi + Speaker.
set -u
STEP=${1:-precheck}
[ "$STEP" = "--" ] && STEP=${2:-precheck}
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
CARD=F6
TMP=$(mktemp -d /tmp/fp6-audio-test.XXXXXX)
ORIG_VOL=""
pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*"; FAILED=1; }
info() { echo "INFO: $*"; }
FAILED=0

ampvol() { amixer -c $CARD cget "name=Amplifier $1 PCM Playback Volume" | awk -F= '/: values=/{print $2}'; }
ampprof() { amixer -c $CARD cget "name=Amplifier $1 Profile Set" | awk -F= '/: values=/{print $2}'; }
profname() { case "$1" in 0) echo music;; 1) echo receiver;; 2) echo voice;; *) echo "?$1";; esac; }

guard_amps() {
  local l r; l=$(ampvol L); r=$(ampvol R)
  info "amp volumes L=$l R=$r profiles L=$(profname "$(ampprof L)") R=$(profname "$(ampprof R)")"
  if [ -z "$l" ] || [ -z "$r" ] || [ "$l" -gt 352 ] || [ "$r" -gt 352 ]; then
    echo "ABORT: amplifier volume above 352 or unreadable; not playing anything" >&2
    exit 3
  fi
}

save_vol() { ORIG_VOL=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{print $2}'); }
restore() {
  [ -n "$ORIG_VOL" ] && wpctl set-volume @DEFAULT_AUDIO_SINK@ "$ORIG_VOL" >/dev/null 2>&1
  rm -rf "$TMP"
}
trap restore EXIT

# Card device id, profiles and routes from pw-dump (no pactl on the phone).
pwinfo() {
  pw-dump | python3 -c '
import json,sys
want=sys.argv[1]
for o in json.load(sys.stdin):
  if o.get("type","").endswith(":Device") and o.get("info",{}).get("props",{}).get("device.name")=="alsa_card.platform-sound":
    p=o["info"]["params"]
    if want=="id": print(o["id"])
    elif want=="profiles":
      for x in p.get("EnumProfile",[]): print(x["index"], x["name"], x.get("priority"))
    elif want=="active-profile":
      for x in p.get("Profile",[]): print(x["index"], x["name"])
    elif want=="routes":
      for x in p.get("EnumRoute",[]): print(x["index"], x["direction"], x["name"], ",".join(map(str,x.get("devices",[]))))
    elif want=="active-routes":
      for x in p.get("Route",[]): print(x["index"], x["direction"], x["name"], x.get("device"))
' "$1"
}
DEV=$(pwinfo id)

set_profile() { # name, or a UCM verb ("Voice Call") -> its highest-priority "Verb (devices)" profile
  local idx; idx=$(pwinfo profiles | awk -v n="$1" '{i=$1; p=$NF; $1=""; $NF=""; sub(/^ /,""); sub(/ $/,""); if (($0==n || index($0, n " (")==1) && (best=="" || p>bp)) {best=i; bp=p}} END {if (best!="") print best}')
  [ -n "$idx" ] || { fail "profile '$1' not offered"; return 1; }
  wpctl set-profile "$DEV" "$idx"; sleep 2
}
set_route() { # "[Out] Earpiece"
  local line idx d
  line=$(pwinfo routes | awk -v n="$1" '{i=$1; dir=$2; $1=""; $2=""; devs=$NF; $NF=""; gsub(/^ +| +$/,""); if ($0==n) print i, devs}')
  [ -n "$line" ] || { fail "route '$1' not offered in profile $(pwinfo active-profile)"; return 1; }
  idx=${line% *}; d=${line#* }; d=${d%%,*}
  pw-cli set-param "$DEV" Route "{ index: $idx, device: $d, save: false }" >/dev/null; sleep 1
}

tone() { # file dBFS seconds
  python3 - "$1" "$2" "$3" <<'PY'
import math,struct,sys
f,db,sec=sys.argv[1],float(sys.argv[2]),float(sys.argv[3])
a=10**(db/20)*32767; n=int(48000*sec); fade=2400
with open(f,"wb") as o:
  for i in range(n):
    g=min(1,i/fade,(n-i)/fade)
    v=int(a*g*math.sin(2*math.pi*440*i/48000)); o.write(struct.pack("<hh",v,v))
PY
}
play_tone() { # dBFS sinkvol
  guard_amps
  save_vol
  wpctl set-volume @DEFAULT_AUDIO_SINK@ "$2"
  tone "$TMP/t.raw" "$1" 2
  pw-play --raw --rate 48000 --channels 2 --format s16 "$TMP/t.raw"
  local rc=$?
  wpctl set-volume @DEFAULT_AUDIO_SINK@ "$ORIG_VOL"
  return $rc
}

rms() { # seconds -> prints dBFS rms and peak
  pw-record --raw --rate 48000 --channels 1 --format s16 "$TMP/m.raw" & local p=$!
  sleep "$1"; kill -INT $p; wait $p 2>/dev/null
  python3 - "$TMP/m.raw" <<'PY'
import math,struct,sys
d=open(sys.argv[1],"rb").read(); n=len(d)//2
if n < 4800: print("nan nan 0"); sys.exit()
s=struct.unpack("<%dh"%n,d[:2*n])[4800:]   # skip first 100 ms
r=math.sqrt(sum(x*x for x in s)/len(s)) or 1e-9; pk=max(abs(x) for x in s) or 1
print("%.1f %.1f %d"%(20*math.log10(r/32768),20*math.log10(pk/32768),len(s)))
PY
}

step_precheck() {
  echo "== precheck (read-only)"
  cat /proc/asound/cards
  local ucm; ucm=$(tr '\0' '\n' < /proc/$(pgrep -u "$(id -u)" -x wireplumber | head -1)/environ | sed -n 's/^ALSA_CONFIG_UCM2=//p')
  info "wireplumber ALSA_CONFIG_UCM2=$ucm"
  case "$ucm" in *alsa-ucm-conf-fp6-feature*) pass "feature UCM active";; *) fail "feature UCM not active (old generation?)";; esac
  ALSA_CONFIG_UCM2=$ucm alsaucm -c hw:0 list _verbs
  # list/get only: these never apply UCM sequences
  ALSA_CONFIG_UCM2=$ucm alsaucm -c hw:0 list _devices/HiFi
  ALSA_CONFIG_UCM2=$ucm alsaucm -c hw:0 list "_devices/Voice Call"
  info "profiles:"; pwinfo profiles
  info "active profile: $(pwinfo active-profile)"; info "routes:"; pwinfo routes
  pwinfo profiles | grep -q " Voice Call " && pass "Voice Call profile offered" || fail "no Voice Call profile"
  [ "$(pwinfo profiles | grep -c -E '^[0-9]+ (HiFi|Voice Call)( |$)')" -ge 1 ] && pass "unsplit HiFi/Voice Call profiles" || info "profiles are split per device (old UCM with ConflictingDevice)"
  pwinfo routes | grep -qF "[In] Mic2" && info "Mic2 exposed (topMic = true)" || info "Mic2 not exposed (default; top mic disabled)"
  for r in "[Out] Speaker" "[Out] Earpiece" "[In] Mic1"; do
    pwinfo routes | grep -qF "$r" && pass "route $r" || fail "route $r missing"
  done
  guard_amps
  wpctl status | sed -n '/^Audio/,/^Video/p'
}

step_speaker() {
  echo "== speaker: quiet 440 Hz tone, both speakers"
  set_route "[Out] Speaker"
  info "active: $(pwinfo active-routes | tr '\n' ';')"
  [ "$(profname "$(ampprof L)")" = music ] && [ "$(profname "$(ampprof R)")" = music ] && pass "amps music profile" || fail "amp profile not music"
  play_tone -20 0.50 && pass "speaker tone played (operator: heard from top AND bottom?)" || fail "pw-play failed"
  cat /proc/asound/card0/pcm0p/sub0/status 2>/dev/null | head -3
}

step_earpiece() {
  echo "== earpiece: hold phone ~10 cm from ear, NOT against it"
  sleep 3
  set_route "[Out] Earpiece" || return
  local l r p; l=$(ampvol L); r=$(ampvol R); p=$(profname "$(ampprof L)")
  [ "$p" = receiver ] && [ "$r" = 0 ] && [ "$l" -le 352 ] && pass "earpiece mixer: L=$l receiver, R=$r" || fail "earpiece mixer L=$l $p R=$r"
  play_tone -30 0.30 && pass "earpiece tone played (operator: heard only from the top earpiece?)" || fail "pw-play failed"
  set_route "[Out] Speaker"
  [ "$(profname "$(ampprof L)")" = music ] && pass "back on speaker, L music" || fail "L profile not restored to music"
}

mic_common() { # route label
  local label=$1
  set_route "$label" || return
  info "active: $(pwinfo active-routes | tr '\n' ';')"
  info "recording 3 s of room noise (stay quiet)"; set -- $(rms 3); local q=$1
  info "quiet: rms=$1 dBFS peak=$2 dBFS samples=$3"
  info "now speak/tap near the mic for 3 s"; sleep 1; set -- $(rms 3); local s=$1
  info "speech: rms=$1 dBFS peak=$2 dBFS"
  python3 -c "import sys; q,s=float('$q'),float('$s'); sys.exit(0 if (q>-100 and s>q+6 and s<-1) else 1)" \
    && pass "$label: signal present and responds to sound" || fail "mic: no/flat signal (quiet=$q speech=$s)"
}
step_mic()  { echo "== bottom mic (Mic1/AMIC1)"; mic_common "[In] Mic1"; }
# Mic1/Mic2 are conflicting UCM devices, so PipeWire offers them as separate profiles.
step_mic2() {
  echo "== top mic (Mic2/AMIC3; needs topMic = true; reads ADC2 via SWR_MIC0)"
  if ! pwinfo routes | grep -qF "[In] Mic2"; then info "Mic2 not exposed; skipped"; return; fi
  # unsplit UCM: Mic2 is a port of the HiFi source; split UCM: its own profile
  pwinfo profiles | grep -q "HiFi (Mic2" && set_profile "HiFi (Mic2, Speaker)"
  mic_common "[In] Mic2"
  info "second capture on Mic2 (the old failure: zeros/stall here)"; set -- $(rms 3)
  info "Mic2 again: rms=$1 dBFS peak=$2 dBFS samples=$3"
  python3 -c "import sys; sys.exit(0 if (float('$1')>-100 and int('$3')>100000) else 1)" && pass "Mic2 keeps streaming" || fail "Mic2 stalls/zeros on re-capture"
  pwinfo profiles | grep -q "HiFi (Mic1" && set_profile "HiFi (Mic1, Speaker)"
  set_route "[In] Mic1"
}

step_volume() {
  echo "== software volume readback"
  save_vol
  for v in 0.10 0.25 0.40; do
    wpctl set-volume @DEFAULT_AUDIO_SINK@ $v
    got=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{print $2}')
    [ "$got" = "$v" ] && pass "sink volume $v" || fail "set $v got $got"
  done
  wpctl set-mute @DEFAULT_AUDIO_SINK@ 1; wpctl get-volume @DEFAULT_AUDIO_SINK@ | grep -q MUTED && pass mute || fail mute
  wpctl set-mute @DEFAULT_AUDIO_SINK@ 0
  wpctl set-volume @DEFAULT_AUDIO_SINK@ "$ORIG_VOL"
  info "amp volumes unchanged by software volume: L=$(ampvol L) R=$(ampvol R)"
}

step_keys() {
  echo "== volume keys (Lomiri session must be running): press VOL+ twice, then VOL- once, within 10 s"
  save_vol; local before=$ORIG_VOL
  sleep 10
  local after; after=$(wpctl get-volume @DEFAULT_AUDIO_SINK@ | awk '{print $2}')
  info "sink volume before=$before after=$after (expect +1 net step)"
  [ "$before" != "$after" ] && pass "volume keys change sink volume" || fail "no change (indicator-sound / key mapping?)"
  ORIG_VOL=""   # keep the user's new volume
}

step_usb() {
  echo "== USB-C audio (digital adapters/headsets only; FP6 has no analog USB-C audio)"
  info "use Wi-Fi SSH: plugging a USB-C adapter switches the port to host mode"
  info "typec: data_role=$(cat /sys/class/typec/port0/data_role) power_role=$(cat /sys/class/typec/port0/power_role)"
  info "plug the USB-C headset/adapter now; waiting 20 s"; sleep 20
  info "typec after: data_role=$(cat /sys/class/typec/port0/data_role)"
  cat /proc/asound/cards
  lsmod | grep -q snd_usb_audio && pass "snd-usb-audio loaded" || fail "snd-usb-audio not loaded"
  wpctl status | sed -n '/^Audio/,/^Video/p'
  info "if a USB sink is default: run 'speaker' style tone manually at low volume with headphones OFF the ears first"
  info "the 'Fairphone (Gen. 6) Headset Jack' input never reports (no WCD9378 MBHC; no analog jack) - expected"
}

step_callroute() {
  echo "== call routing (no call placed): Voice Call profile, earpiece/speaker, restore"
  set_profile "Voice Call" || return
  info "active: $(pwinfo active-profile) / $(pwinfo active-routes | tr '\n' ';')"
  sleep 1
  local p; p=$(profname "$(ampprof L)")
  [ "$p" = receiver ] && [ "$(ampvol R)" = 0 ] && pass "Voice Call default = earpiece (L receiver, R muted)" || fail "Voice Call default route L=$p R=$(ampvol R)"
  # PipeWire splits same-PCM UCM devices into "Voice Call (<devices>)"
  # profiles; speakerphone is then a profile switch (what the patched
  # telephony-service does), otherwise a port switch.
  if pwinfo profiles | grep -q "Voice Call (.*Speaker"; then
    local sp; sp=$(pwinfo profiles | awk '/Voice Call \(.*Speaker/ {i=$1; p=$NF; if (best=="" || p>bp) {best=i; bp=p}} END {print best}')
    info "split profiles: switching to profile index $sp for speakerphone"
    wpctl set-profile "$DEV" "$sp"; sleep 2
    info "active: $(pwinfo active-profile)"
  else
    set_route "[Out] Speaker"
  fi
  [ "$(profname "$(ampprof L)")" = voice ] && [ "$(profname "$(ampprof R)")" = voice ] && pass "speakerphone: both amps voice profile" || fail "speakerphone profiles L=$(profname "$(ampprof L)") R=$(profname "$(ampprof R)")"
  guard_amps
  set_profile "HiFi"
  pwinfo routes | grep -qF "[Out] Speaker" && set_route "[Out] Speaker"
  [ "$(profname "$(ampprof L)")" = music ] && [ "$(profname "$(ampprof R)")" = music ] && pass "restored HiFi, music profiles" || fail "restore: L=$(profname "$(ampprof L)") R=$(profname "$(ampprof R)")"
  guard_amps
}

case "$STEP" in
  precheck|speaker|earpiece|mic|mic2|volume|keys|usb|callroute) "step_$STEP" ;;
  all) step_precheck; step_volume; step_callroute; step_speaker; step_earpiece; step_mic; step_mic2 ;;
  *) echo "unknown step $STEP" >&2; exit 2 ;;
esac
echo "RESULT: $([ $FAILED = 0 ] && echo PASS || echo FAIL) ($STEP)"
exit $FAILED
