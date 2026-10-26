{ config, lib, pkgs, ... }:
# FP6 microSD auto-mount for Lomiri's file manager.
#
# lomiri-filemanager-app (src/plugin/placesmodel/placesmodel.cpp) lists
# "external drives" from mtab entries under /media/$USER (or the runtime dir).
# NixOS udisks2 mounts under /run/media/$USER, and mountOnMedia gives
# /media/<label>; neither appears in Lomiri's Places. Lomiri has no session
# automounter either (that was Ubuntu Touch platform glue).
#
# This module mounts ONLY partitions of the removable SD host
# (ID_PATH platform-8804000.mmc, sdhci_msm "sdhc_2") at /media/<user>/<label>
# via systemd-mount. The mount unit is bound to the block device, so pulling
# the card stops it. UFS (sda..sdf), loop and zram devices never match.
let
  cfg = config.services.fp6SdAutomount;
  user = cfg.user;
  uid = toString config.users.users.${user}.uid;
  gid = toString config.users.groups.${config.users.users.${user}.group}.gid;
  mounter = pkgs.writeShellApplication {
    name = "fp6-sd-mount";
    runtimeInputs = [ pkgs.coreutils pkgs.util-linux pkgs.systemd pkgs.gnused ];
    text = ''
      dev=/dev/''${1:?kernel name}
      [[ "$1" =~ ^mmcblk[0-9]+(p[0-9]+)?$ ]] || { echo "refusing non-mmc device $1" >&2; exit 1; }
      [[ -b "$dev" ]] || exit 0
      path=$(udevadm info -q property -n "$dev" | sed -n 's/^ID_PATH=//p')
      [[ "$path" == ${lib.escapeShellArg cfg.idPath}* ]] || { echo "refusing $dev on $path" >&2; exit 1; }
      findmnt -rn -S "$dev" >/dev/null && { echo "$dev already mounted"; exit 0; }
      fstype=$(blkid -o value -s TYPE "$dev" || true)
      label=$(blkid -o value -s LABEL "$dev" || true)
      uuid=$(blkid -o value -s UUID "$dev" || true)
      name=$(printf '%s' "''${label:-''${uuid:-$1}}" | tr -c 'A-Za-z0-9._-' '_' | cut -c1-48)
      base=/media/${user}
      mkdir -p /media && chmod 0755 /media
      install -d -m 0750 -o root -g ${gid} "$base"
      target="$base/$name"; n=1
      while findmnt -rn "$target" >/dev/null; do target="$base/$name-$n"; n=$((n+1)); done
      case "$fstype" in
        vfat|exfat) opts="rw,nosuid,nodev,noexec,relatime,uid=${uid},gid=${gid},fmask=0077,dmask=0077,flush" ;;
        ext4|ext3|ext2|f2fs|btrfs) opts="rw,nosuid,nodev,noexec,relatime" ;;
        *) echo "unsupported filesystem '$fstype' on $dev; not mounting"; exit 0 ;;
      esac
      ${lib.optionalString cfg.readOnly ''opts="''${opts/rw,/ro,}"''}
      # systemd-mount creates a transient mount unit BindsTo= the device.
      systemd-mount --no-block --collect -t "$fstype" -o "$opts" "$dev" "$target"
      echo "mounted $dev ($fstype) at $target"
    '';
  };
in {
  options.services.fp6SdAutomount = {
    enable = lib.mkEnableOption "FP6 microSD automount under /media/<user> for Lomiri file manager";
    user = lib.mkOption { type = lib.types.str; default = "user"; };
    idPath = lib.mkOption { type = lib.types.str; default = "platform-8804000.mmc"; description = "udev ID_PATH prefix of the removable SD host."; };
    readOnly = lib.mkOption { type = lib.types.bool; default = false; description = "Mount read-only (first live test)."; };
  };
  config = lib.mkIf cfg.enable {
    services.udisks2.enable = true;
    environment.systemPackages = [ mounter pkgs.exfatprogs pkgs.dosfstools ];
    system.build.fp6-sd-mount = mounter;
    services.udev.extraRules = ''
      # FP6 microSD: hand filesystems on the removable SD host to fp6-sd-mount
      ACTION=="add", SUBSYSTEM=="block", KERNEL=="mmcblk[0-9]*", ENV{ID_PATH}=="${cfg.idPath}*", ENV{ID_FS_USAGE}=="filesystem", ENV{SYSTEMD_WANTS}+="fp6-sd-mount@%k.service"
      # Keep udisks2 from offering a second /run/media mount of the same card
      SUBSYSTEM=="block", KERNEL=="mmcblk[0-9]*", ENV{ID_PATH}=="${cfg.idPath}*", ENV{UDISKS_AUTO}="0"
    '';
    systemd.services."fp6-sd-mount@" = {
      description = "FP6 microSD mount for %I";
      after = [ "local-fs.target" ];
      serviceConfig = { Type = "oneshot"; ExecStart = "${lib.getExe mounter} %I"; TimeoutSec = 30; };
    };
    # Clean stale empty mountpoints left by an unclean card removal.
    systemd.tmpfiles.rules = [ "d /media 0755 root root -" ];
  };
}
