#!/usr/bin/env bash
# fp6-rmtfs-shadow: boot-time guard, confirmation and reset for rmtfs -r -D
# (persistent read-only EFS shadow; see rmtfs-shadow-fp6.nix).
#
#   prestart  rmtfs ExecStartPre: handle RESET, count attempts with saved
#             images, fall back to the read-only partitions after 2 starts
#             that were never confirmed, and point the modem's TFTP scratch
#             (/run/tqftpserv) at storage that lives exactly as long as the
#             images.
#   confirm   after fp6-modem-mcfg-activate: platform MCFG Active -> the
#             modem came up with these images; clear the attempt counter.
#   reset     forget the saved images (next start = plain read-only rmtfs,
#             which then captures afresh).
#   status    show what is stored (never prints image contents).
#
# The real modem partitions are never touched by this tool or by rmtfs -D.
set -euo pipefail

DIR="${FP6_RMTFS_SHADOW_DIR:-/var/lib/rmtfs-shadow}"
SCRATCH_LINK="${FP6_TQFTP_SCRATCH:-/run/tqftpserv}"
MAX_ATTEMPTS="${FP6_RMTFS_SHADOW_MAX_ATTEMPTS:-2}"
COUNTER="$DIR/boot-attempts"
QMICLI="${FP6_RMTFS_SHADOW_QMICLI:-qmicli}"   # overridable for tests

log() { echo "fp6-rmtfs-shadow: $*"; }

images() {
  find "$DIR" -maxdepth 1 -type f -name '*.img' -printf '%f\n' 2>/dev/null | sort
}

read_counter() {
  local n
  n=$(cat "$COUNTER" 2>/dev/null || echo 0)
  case "$n" in ''|*[!0-9]*) n=0 ;; esac
  echo "$n"
}

write_counter() {
  echo "$1" > "$COUNTER.tmp"
  sync "$COUNTER.tmp" 2>/dev/null || true
  mv -f "$COUNTER.tmp" "$COUNTER"
}

drop_scratch() {
  rm -rf "$DIR/readwrite"
}

point_scratch() {
  install -d -m 0700 "$DIR/readwrite"
  rm -rf "$SCRATCH_LINK"
  ln -sfn "$DIR/readwrite" "$SCRATCH_LINK"
}

cmd_prestart() {
  install -d -m 0700 "$DIR"
  chmod 0700 "$DIR"
  rm -f "$DIR"/*.img.tmp

  if [ -e "$DIR/RESET" ]; then
    rm -f "$DIR"/*.img "$DIR/RESET"
    drop_scratch
    write_counter 0
    log "reset requested: saved images removed, starting from the read-only partitions"
  fi

  if [ -z "$(images)" ]; then
    # Fresh EFS (from the partitions): the scratch must be fresh too, or a
    # stale mcfg.tmp makes the modem skip MCFG selection.
    drop_scratch
    write_counter 0
    log "no saved images: the modem starts from the read-only partitions"
  else
    local n
    n=$(read_counter)
    if [ "$n" -ge "$MAX_ATTEMPTS" ]; then
      local q
      q="$DIR/quarantine/$(date +%Y%m%dT%H%M%S)"
      install -d -m 0700 "$DIR/quarantine" "$q"
      mv -f "$DIR"/*.img "$q"/
      drop_scratch
      write_counter 0
      log "FALLBACK: $n starts with saved images were never confirmed; moved them to $q, starting from the read-only partitions"
    else
      write_counter $((n + 1))
      log "loading saved images ($(images | tr '\n' ' ')), attempt $((n + 1))/$MAX_ATTEMPTS until confirmed"
    fi
  fi

  # Remove old quarantines beyond the newest 3.
  if [ -d "$DIR/quarantine" ]; then
    find "$DIR/quarantine" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -r | tail -n +4 |
      while read -r d; do rm -rf "${DIR:?}/quarantine/$d"; done
  fi

  point_scratch
}

cmd_confirm() {
  local wait="${FP6_RMTFS_SHADOW_CONFIRM_WAIT:-120}" st=""
  if [ "$(read_counter)" = 0 ]; then
    log "nothing to confirm"
    return 0
  fi
  local deadline=$((SECONDS + wait))
  while [ "$SECONDS" -lt "$deadline" ]; do
    st=$(timeout -k 5 30 "$QMICLI" --silent -d qrtr://0 --pdc-list-configs=platform 2>/dev/null |
      grep -o "Status: *[A-Za-z]*" | awk '{print $2}' | sort -u | tr '\n' ' ' || true)
    case " $st " in
      *" Active "*)
        write_counter 0
        log "modem up with the saved images (platform MCFG Active); confirmed"
        return 0 ;;
    esac
    sleep 10
  done
  log "NOT confirmed: platform MCFG not Active (${st:-no answer}); attempt counter stays at $(read_counter)/$MAX_ATTEMPTS"
  return 0
}

cmd_reset() {
  [ -d "$DIR" ] || { log "nothing stored"; return 0; }
  # RESET first, so a running rmtfs stops saving before the images go.
  : > "$DIR/RESET"
  rm -f "$DIR"/*.img "$DIR"/*.img.tmp
  log "saved images removed; rmtfs stops saving now. Reboot to run from the read-only partitions (rmtfs then captures afresh). Originals kept in $DIR/original/."
}

cmd_status() {
  echo "directory:      $DIR"
  echo "saved images:   $(images | tr '\n' ' ')"
  echo "attempts:       $(read_counter)/$MAX_ATTEMPTS (0 = confirmed or nothing loaded)"
  echo "reset pending:  $([ -e "$DIR/RESET" ] && echo yes || echo no)"
  echo "originals:      $(find "$DIR/original" -maxdepth 1 -type f -name '*.img' -printf '%f ' 2>/dev/null)"
  echo "quarantined:    $(find "$DIR/quarantine" -mindepth 1 -maxdepth 1 -type d -printf '%f ' 2>/dev/null)"
  echo "tftp scratch:   $(readlink "$SCRATCH_LINK" 2>/dev/null || echo "$SCRATCH_LINK (not linked)")"
}

case "${1:-}" in
  prestart) cmd_prestart ;;
  confirm) cmd_confirm ;;
  reset) cmd_reset ;;
  status) cmd_status ;;
  *) echo "usage: fp6-rmtfs-shadow {status|reset|prestart|confirm}" >&2; exit 2 ;;
esac
