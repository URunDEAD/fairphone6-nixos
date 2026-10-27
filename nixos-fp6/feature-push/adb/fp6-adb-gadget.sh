# fp6-adb-gadget prepare|bind|unbind|sync   (root; called by fp6-adbd.service)
#
# Adds/removes the ADB FunctionFS function in the existing configfs gadget g1
# (NCM usb0 stays in c.1 the whole time):
#   prepare  ExecStartPre: (re)mount /dev/usb-ffs/adb owned by $ADBD_USER so
#            the unprivileged adbd can open ep0/ep1/ep2.
#   bind     ExecStartPost: wait for adbd's descriptors, link ffs.adb into c.1
#            and rebind the UDC (USB re-enumerates: ~1 s NCM blip).
#   unbind   ExecStopPost: drop the ffs.adb link, umount, rebind NCM only.
#   sync     boot (Developer mode off): remove a leftover stage-1 adb link and
#            make sure the UDC is bound with NCM only.
# FunctionFS unbinds the whole gadget when its daemon closes ep0, so every
# path ends with the UDC bound again.
set -euo pipefail
op=${1:?prepare|bind|unbind|sync}
gadget=/sys/kernel/config/usb_gadget/g1
function_dir=$gadget/functions/ffs.adb
link=$gadget/configs/c.1/adb
endpoints=/dev/usb-ffs/adb
user=${ADBD_USER:?ADBD_USER is not set}
uid=$(id -u "$user")
gid=$(id -g "$user")
log() { echo "fp6-adb-gadget $op: $*"; }

[[ -d $gadget/configs/c.1 ]] || { log "no configfs gadget g1"; exit 1; }
exec 9>/run/fp6-adb-gadget.lock
flock -w 30 9

controller() {
  local c
  for c in /sys/class/udc/*; do
    [[ -e $c ]] && { basename "$c"; return 0; }
  done
  log "no UDC"; exit 1
}
bound() { [[ -n "$(tr -d '[:space:]' < "$gadget/UDC")" ]]; }
udc_unbind() { if bound; then printf '\n' > "$gadget/UDC"; fi; }
linked() { [[ -n "$(find "$gadget/configs/c.1" -maxdepth 1 -type l -print -quit)" ]]; }
udc_bind() {
  local c
  # No function left (usb-modes "charging only"): an empty config cannot bind.
  linked || return 0
  c=$(controller)
  bound || printf '%s\n' "$c" > "$gadget/UDC"
}
drop_link() {
  if [[ -L $link ]]; then
    udc_unbind
    rm "$link"
  fi
}
unmount_ffs() {
  if mountpoint -q "$endpoints"; then umount "$endpoints"; fi
}

case $op in
prepare)
  modprobe usb_f_fs 2>/dev/null || true
  # A link left from stage 1 or a crash refers to endpoints nobody serves.
  drop_link
  unmount_ffs
  udc_bind
  mkdir -p "$function_dir" "$endpoints"
  mount -t functionfs -o "uid=$uid,gid=$gid,rmode=0750,fmode=0600" adb "$endpoints"
  log "FunctionFS mounted for $user"
  ;;
bind)
  for _ in $(seq 1 100); do
    [[ -e $endpoints/ep1 && -e $endpoints/ep2 ]] && break
    sleep 0.1
  done
  [[ -e $endpoints/ep1 && -e $endpoints/ep2 ]] || { log "adbd did not write its descriptors"; exit 1; }
  udc_unbind
  [[ -L $link ]] || ln -s "$function_dir" "$link"
  udc_bind
  log "NCM + ADB bound on $(cat "$gadget/UDC")"
  ;;
unbind|sync)
  drop_link
  unmount_ffs
  if [[ -d $function_dir ]]; then rmdir "$function_dir" 2>/dev/null || true; fi
  udc_bind
  log "without ADB: links $(find "$gadget/configs/c.1" -maxdepth 1 -type l -printf '%f '), UDC '$(cat "$gadget/UDC")'"
  ;;
*)
  log "unknown operation"; exit 2 ;;
esac
