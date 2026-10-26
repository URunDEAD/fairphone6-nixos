{ config, lib, pkgs, ... }:
# FP6 "reset to factory defaults" for NixOS: wipe USER STATE, keep the SYSTEM.
#
# On NixOS the system is /nix/store + the generation symlinks; /etc is
# regenerated at activation. "Factory state" = the same generation with no
# user data. What is removed (or, in the default "quarantine" mode, moved
# aside to /var/lib/fp6-factory-reset/quarantine-<UTC>/ on the same
# filesystem, which is instant and reversible):
#
#   /home/<user>                       (recreated empty with the right owner)
#   /var/lib/NetworkManager, /etc/NetworkManager/system-connections
#   /var/lib/bluetooth, /var/lib/AccountsService/users/<user> (icons stay)
#   /var/lib/upower, /var/lib/usermetrics, /var/lib/geoclue
#   /var/lib/waydroid                  (only if listed in extraPaths; off by default)
#   /var/log/journal                   (persistent journal)
#
# Never touched: /nix, /var/lib/nixos (uid/gid map!), /etc/machine-id,
# /etc/ssh/ssh_host_* (the PC pins this host key), /etc/nixos-secrets
# (login password file), /var/lib/fp6-rtc-offset, boot partitions, /run/nura.
#
# Guard: runs ONLY on the boot after a request, and only if
#   * services.fp6FactoryReset.enable = true in the booted generation, and
#   * /var/lib/fp6-factory-reset/request exists, contains the exact line
#       ERASE-USER-DATA <machine-id> <request-boot-id>
#     where request-boot-id != the current boot id (so it can never act in
#     the same boot it was requested in), and
#   * no target path is, or contains, a mount point (Waydroid/loop safety).
# The request file is consumed (renamed) before any change, so a crash
# mid-reset cannot loop. Request with:  sudo fp6-factory-reset request
let
  cfg = config.services.fp6FactoryReset;
  stateDir = "/var/lib/fp6-factory-reset";
  user = cfg.user;
  home = config.users.users.${user}.home;
  group = config.users.users.${user}.group;
  targets = [
    home
    "/var/lib/NetworkManager"
    "/etc/NetworkManager/system-connections"
    "/var/lib/bluetooth"
    "/var/lib/AccountsService/users/${user}"
    "/var/lib/upower"
    "/var/lib/usermetrics"
    "/var/lib/geoclue"
    "/var/log/journal"
  ] ++ cfg.extraPaths;
  never = [ "/" "/nix" "/nix/store" "/var/lib/nixos" "/etc/ssh" "/etc/nixos-secrets" "/run/nura" "/boot" "/etc" "/var" "/var/lib" ];
  tool = pkgs.writeShellApplication {
    name = "fp6-factory-reset";
    runtimeInputs = [ pkgs.coreutils pkgs.util-linux pkgs.findutils pkgs.gawk ];
    text = ''
      state=${stateDir}
      req=$state/request
      mid=$(cat /etc/machine-id)
      bid=$(cat /proc/sys/kernel/random/boot_id)
      targets=(${lib.escapeShellArgs targets})
      never=(${lib.escapeShellArgs never})
      mode=${cfg.mode}
      log() { echo "fp6-factory-reset: $*"; }
      check_targets() {
        local t n
        for t in "''${targets[@]}"; do
          for n in "''${never[@]}"; do [[ "$t" == "$n" ]] && { log "refusing protected path $t"; return 1; }; done
          [[ "$t" == /* && "$t" != *..* ]] || { log "bad path $t"; return 1; }
          [[ -e "$t" ]] || continue
          if findmnt -rn -o TARGET | awk -v t="$t" '$0==t || index($0, t"/")==1 {f=1} END {exit !f}'; then
            log "refusing: $t is or contains a mount point"; return 1
          fi
        done
      }
      case "''${1:-}" in
        plan)
          log "mode=$mode; would ${if cfg.mode == "erase" then "ERASE" else "move aside"}:"
          for t in "''${targets[@]}"; do [[ -e "$t" ]] && printf '  %s (%s)\n' "$t" "$(du -sh --one-file-system "$t" 2>/dev/null | cut -f1)"; done
          check_targets && log "mount-point check: OK"
          ;;
        request)
          [[ $(id -u) == 0 ]] || { log "must run as root"; exit 1; }
          check_targets
          [[ "''${2:-}" == --yes-erase-user-data ]] || { log "re-run with: fp6-factory-reset request --yes-erase-user-data   (then reboot)"; exit 1; }
          mkdir -p "$state"; printf 'ERASE-USER-DATA %s %s\n' "$mid" "$bid" > "$req.tmp"; mv "$req.tmp" "$req"
          log "requested; the reset runs on the NEXT boot. Cancel with: fp6-factory-reset cancel"
          ;;
        cancel)
          rm -f "$req" && log "cancelled"
          ;;
        run)
          [[ -e "$req" ]] || exit 0
          read -r word rmid rbid < "$req" || true
          mv "$req" "$state/consumed-$(date -u +%Y%m%dT%H%M%SZ)"
          [[ "$word" == ERASE-USER-DATA && "$rmid" == "$mid" ]] || { log "malformed/foreign request; ignored"; exit 0; }
          [[ "$rbid" != "$bid" ]] || { log "request from this same boot; ignored"; exit 0; }
          check_targets || { log "aborted, nothing changed"; exit 0; }
          q="$state/quarantine-$(date -u +%Y%m%dT%H%M%SZ)"
          for t in "''${targets[@]}"; do
            [[ -e "$t" ]] || continue
            if [[ "$mode" == erase ]]; then
              rm -rf --one-file-system -- "$t"; log "erased $t"
            else
              mkdir -p "$q$(dirname "$t")"; mv -- "$t" "$q$t"; log "moved $t -> $q$t"
            fi
          done
          install -d -m 0700 -o ${user} -g ${group} ${home}
          log "done; user state reset (mode=$mode)"
          ;;
        *) echo "usage: fp6-factory-reset plan | request --yes-erase-user-data | cancel" >&2; exit 2 ;;
      esac
    '';
  };
in {
  options.services.fp6FactoryReset = {
    enable = lib.mkEnableOption "FP6 factory reset of user state on the boot after an explicit request";
    user = lib.mkOption { type = lib.types.str; default = "user"; };
    mode = lib.mkOption {
      type = lib.types.enum [ "quarantine" "erase" ];
      default = "quarantine";
      description = "quarantine = move aside on the same filesystem (reversible); erase = delete.";
    };
    extraPaths = lib.mkOption { type = lib.types.listOf lib.types.str; default = [ ]; example = [ "/var/lib/waydroid" ]; };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ tool ];
    system.build.fp6-factory-reset = tool;
    systemd.tmpfiles.rules = [ "d ${stateDir} 0700 root root -" ];
    systemd.services.fp6-factory-reset = {
      description = "FP6 factory reset of user state (only after explicit request)";
      wantedBy = [ "sysinit.target" ];
      # Not after systemd-tmpfiles-setup: that is ordered after journal-flush,
      # and this unit must run before journal-flush (it wipes /var/log/journal).
      # The cycle made systemd drop tmpfiles-setup, so /run/opengl-driver was
      # missing and Mir found no graphics platform. The request file can only
      # exist once stateDir does, so tmpfiles is not needed here.
      after = [ "local-fs.target" ];
      before = [ "sysinit.target" "systemd-user-sessions.service" "display-manager.service" "NetworkManager.service"
                 "bluetooth.service" "waydroid-container.service" "systemd-journal-flush.service" "accounts-daemon.service" ];
      unitConfig = { DefaultDependencies = false; ConditionPathExists = "${stateDir}/request"; };
      serviceConfig = { Type = "oneshot"; ExecStart = "${lib.getExe tool} run"; TimeoutSec = 300; };
    };
  };
}
