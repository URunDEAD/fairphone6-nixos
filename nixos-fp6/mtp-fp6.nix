{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6Mtp;
  responder = pkgs.callPackage ./umtprd.nix {};
  mediaDirectories = [ "Documents" "Downloads" "Music" "Pictures" "Public" "Videos" ];
  mtpConfig = pkgs.writeText "fp6-umtprd.conf" (''
    usb_functionfs_mode 0x1
    usb_dev_path "/dev/usb-ffs/mtp/ep0"
    usb_epin_path "/dev/usb-ffs/mtp/ep1"
    usb_epout_path "/dev/usb-ffs/mtp/ep2"
    usb_epint_path "/dev/usb-ffs/mtp/ep3"
    usb_max_packet_size 0x200
    loop_on_disconnect 1
    default_uid 10000
    default_gid 100
    umask 077
    manufacturer "Fairphone"
    product "Fairphone 6"
  '' + lib.concatMapStrings (directory:
    ''storage "/home/user/${directory}" "${directory}" "rw"'' + "\n"
  ) mediaDirectories);
  runMtp = pkgs.writeShellApplication {
    name = "fp6-mtp";
    runtimeInputs = [ pkgs.coreutils pkgs.gnugrep pkgs.util-linux responder ];
    text = ''
      fail() { echo "FP6 MTP: $*" >&2; exit 1; }
      gadget=/sys/kernel/config/usb_gadget/g1
      endpoints=/dev/usb-ffs/mtp
      function_dir=$gadget/functions/ffs.mtp
      function_link=$gadget/configs/c.1/mtp
      mtp_pid=
      mtp_changed=0
      controller=
      cleanup() {
        status=$?
        trap - EXIT INT TERM
        if [[ "$mtp_changed" == 1 ]]; then
          flock -w 30 9 || status=1
          printf '\n' > "$gadget/UDC" || status=1
          if [[ -n "$mtp_pid" ]]; then
            kill "$mtp_pid" 2>/dev/null || true
            wait "$mtp_pid" 2>/dev/null || true
          fi
          if [[ -L "$function_link" && "$(readlink -f "$function_link")" == "$function_dir" ]]; then
            rm "$function_link" || status=1
          fi
          if mountpoint -q "$endpoints"; then
            umount "$endpoints" || status=1
          fi
          rmdir "$endpoints" "$function_dir" 2>/dev/null || status=1
          # Keep the existing NCM (and ADB, if Developer mode is on) functions on every exit.
          printf '%s\n' "$controller" > "$gadget/UDC" || status=1
          echo 'FP6 MTP: original gadget restored'
        fi
        exit "$status"
      }
      trap cleanup EXIT
      trap 'exit 0' INT TERM

      grep -aq 'fairphone,fp6' /sys/firmware/devicetree/base/compatible || fail 'wrong board'
      [[ "$(cat "$gadget/idVendor")" == 0x18d1 ]] || fail 'unexpected USB vendor'
      [[ "$(cat "$gadget/idProduct")" == 0x4ee3 ]] || fail 'unexpected USB product'
      [[ "$(readlink -f "$gadget/configs/c.1/rndis")" == "$gadget/functions/ncm.usb0" ]] || fail 'NCM configuration changed'
      # ADB is optional (Developer mode, feature-push/adb/adb-fp6.nix); if present it must be ffs.adb.
      if [[ -L "$gadget/configs/c.1/adb" ]]; then
        [[ "$(readlink -f "$gadget/configs/c.1/adb")" == "$gadget/functions/ffs.adb" ]] || fail 'ADB configuration changed'
      fi
      [[ ! -e "$function_dir" && ! -L "$function_link" && ! -e "$endpoints" ]] || fail 'MTP resources already exist'
      controller=$(cat "$gadget/UDC")
      [[ -n "$controller" && -d "/sys/class/udc/$controller" ]] || fail 'USB controller is not bound'
      for directory in ${lib.escapeShellArgs mediaDirectories}; do
        [[ -d "/home/user/$directory" && ! -L "/home/user/$directory" ]] || fail 'media directory missing or symlinked'
      done

      # Shared gadget lock (also taken by adb/fp6-adb-gadget and the usb-drive
      # helper): held only while the gadget is being changed, not while serving.
      exec 9>/run/fp6-adb-gadget.lock
      flock -w 30 9 || fail 'USB gadget is busy'
      mtp_changed=1
      printf '\n' > "$gadget/UDC"
      mkdir "$function_dir" "$endpoints"
      mount -t functionfs mtp "$endpoints"
      ln -s "$function_dir" "$function_link"
      umtprd -conf ${mtpConfig} &
      mtp_pid=$!
      ready=0
      for _ in {1..20}; do
        kill -0 "$mtp_pid" 2>/dev/null || fail 'responder exited before readiness'
        if [[ -e "$endpoints/ep1" && -e "$endpoints/ep2" && -e "$endpoints/ep3" ]]; then
          ready=1
          break
        fi
        sleep 1
      done
      [[ "$ready" == 1 ]] || fail 'FunctionFS descriptors did not become ready'
      printf '%s\n' "$controller" > "$gadget/UDC"
      echo 'FP6 MTP: MTP endpoints bound next to the existing functions'
      flock -u 9
      wait "$mtp_pid"
    '';
  };
in {
  options.services.fp6Mtp.enable = lib.mkEnableOption "FP6 MTP over the existing NCM/ADB gadget";
  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.users.users.user.uid == 10000 && config.users.groups.users.gid == 100;
      message = "FP6 MTP storage ownership must match the phone user";
    }];
    system.build.fp6-mtp = runMtp;
    environment.systemPackages = [ responder ];
    systemd.tmpfiles.rules = map (directory:
      "d /home/user/${directory} 0700 user users -"
    ) mediaDirectories;
    # Start explicitly for USB acceptance. Leave the known stage-1 USB path
    # unchanged; unready FunctionFS endpoints would prevent enumeration.
    systemd.services.fp6-mtp = {
      description = "FP6 USB MTP file transfer";
      after = [ "adbd.service" "fp6-adbd.service" "systemd-tmpfiles-setup.service" ];
      serviceConfig = {
        ExecStart = lib.getExe runMtp;
        Type = "simple";
        Restart = "no";
        KillMode = "mixed";
        TimeoutStopSec = 15;
        NoNewPrivileges = true;
      };
    };
  };
}
