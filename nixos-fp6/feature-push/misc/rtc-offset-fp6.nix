{ config, lib, pkgs, ... }:
# FP6 wall-clock persistence across power-off.
#
# rtc0 is rtc-pm8xxx (qcom,pmk8350-rtc @ pmk8550 0x6100). It keeps counting
# while the phone is off, but Linux cannot set it (no allow-set-time, no nvmem
# "offset" cell, no UEFI rtc-info), so it reads ~1970 + time since the last
# PMIC reset. Android solves this in userspace (time_daemon keeps an offset);
# this module does the same:
#
#   saved state:  "<rtc_raw> <wall_utc>"   (/var/lib/fp6-rtc-offset/state)
#   restore:      wall = rtc_raw_now + (wall_saved - rtc_raw_saved)
#
# Safety rules:
#  * never writes the RTC, never touches nvmem/SDAM;
#  * only moves the clock FORWARD (never undoes timesyncd/NTP);
#  * inert if rtc0 already reads a plausible time (>= 2025-01-01), e.g. if a
#    future kernel enables allow-set-time;
#  * if rtc_raw_now < rtc_raw_saved (battery pulled -> PMIC RTC reset) the
#    offset is invalid and is NOT applied (timesyncd's clock file still
#    provides a monotonic floor);
#  * saves only when the clock is NTP-synchronised, or when this boot was
#    itself restored from a valid offset.
let
  cfg = config.services.fp6RtcOffset;
  stateDir = "/var/lib/fp6-rtc-offset";
  tool = pkgs.writeShellApplication {
    name = "fp6-rtc-offset";
    runtimeInputs = [ pkgs.coreutils pkgs.systemd ];
    text = ''
      rtc=/sys/class/rtc/${cfg.rtc}
      state=${stateDir}/state
      marker=/run/fp6-rtc-offset/restored
      plausible=1735689600   # 2025-01-01T00:00:00Z
      log() { echo "fp6-rtc-offset: $*"; }
      raw() { cat "$rtc/since_epoch"; }
      [[ -r "$rtc/since_epoch" ]] || { log "no $rtc; nothing to do"; exit 0; }
      [[ "$(cat "$rtc/name" 2>/dev/null)" == rtc-pm8xxx* ]] || { log "unexpected RTC driver; inert"; exit 0; }
      case "''${1:-}" in
        restore)
          r=$(raw)
          if (( r >= plausible )); then log "RTC already plausible ($r); inert"; exit 0; fi
          [[ -r "$state" ]] || { log "no saved offset yet"; exit 0; }
          read -r sraw swall < "$state" || { log "unreadable state"; exit 0; }
          [[ "$sraw" =~ ^[0-9]+$ && "$swall" =~ ^[0-9]+$ ]] || { log "malformed state; ignored"; exit 0; }
          if (( r < sraw )); then log "RTC counter went backwards ($r < $sraw): PMIC RTC reset, offset discarded"; exit 0; fi
          target=$(( r + swall - sraw ))
          now=$(date -u +%s)
          if (( target <= now )); then log "clock already at/after offset time ($now >= $target); unchanged"; exit 0; fi
          date -u -s "@$target" >/dev/null
          mkdir -p /run/fp6-rtc-offset && : > "$marker"
          log "clock restored from RTC offset: $(date -u -Is) (+$(( target - now ))s)"
          ;;
        save)
          synced=$(timedatectl show -p NTPSynchronized --value 2>/dev/null || echo no)
          if [[ "$synced" != yes && ! -e "$marker" ]]; then log "clock not synchronised this boot; not saving"; exit 0; fi
          r=$(raw); w=$(date -u +%s)
          (( w >= plausible )) || { log "wall clock implausible ($w); not saving"; exit 0; }
          mkdir -p ${stateDir}
          printf '%s %s\n' "$r" "$w" > "$state.tmp"
          sync "$state.tmp" 2>/dev/null || true
          mv -f "$state.tmp" "$state"
          log "saved offset $(( w - r ))s (raw $r, synced=$synced)"
          ;;
        status)
          r=$(raw); echo "rtc_raw=$r rtc_plausible=$(( r >= plausible ))"
          [[ -r "$state" ]] && { read -r sraw swall < "$state"; echo "saved_raw=$sraw saved_wall=$swall offset=$(( swall - sraw ))"
            echo "predicted_wall=$(date -u -d "@$(( r + swall - sraw ))" -Is) system_wall=$(date -u -Is)"; } || echo "no saved state"
          ;;
        *) echo "usage: fp6-rtc-offset restore|save|status" >&2; exit 2 ;;
      esac
    '';
  };
in {
  options.services.fp6RtcOffset = {
    enable = lib.mkEnableOption "FP6 userspace RTC offset (wall clock survives power-off without NTP)";
    rtc = lib.mkOption { type = lib.types.str; default = "rtc0"; description = "RTC class device (rtc-pm8xxx)."; };
    saveInterval = lib.mkOption { type = lib.types.str; default = "15min"; description = "Periodic save interval while synchronised."; };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ tool ];
    system.build.fp6-rtc-offset = tool;
    systemd.tmpfiles.rules = [ "d ${stateDir} 0755 root root -" ];

    systemd.services.fp6-rtc-offset-restore = {
      description = "FP6: restore wall clock from PMIC RTC + saved offset";
      wantedBy = [ "sysinit.target" ];
      wants = [ "time-set.target" ];
      before = [ "time-set.target" "systemd-timesyncd.service" "sysinit.target" "shutdown.target" ];
      conflicts = [ "shutdown.target" ];
      after = [ "local-fs.target" "systemd-tmpfiles-setup.service" ];
      unitConfig.DefaultDependencies = false;
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${lib.getExe tool} restore";
        # Saving at shutdown keeps the offset fresh even without NTP this boot
        # (only if this boot was restored or synced; see tool).
        ExecStop = "${lib.getExe tool} save";
        TimeoutSec = 20;
      };
    };
    systemd.services.fp6-rtc-offset-save = {
      description = "FP6: save PMIC RTC offset";
      after = [ "time-sync.target" ];
      serviceConfig = { Type = "oneshot"; ExecStart = "${lib.getExe tool} save"; TimeoutSec = 20; };
    };
    systemd.timers.fp6-rtc-offset-save = {
      wantedBy = [ "timers.target" ];
      timerConfig = { OnBootSec = "3min"; OnUnitActiveSec = cfg.saveInterval; AccuracySec = "1min"; };
    };
    # Save as soon as timesyncd reports synchronisation. PathExists would
    # re-trigger the oneshot after every exit and hit the start limit.
    systemd.paths.fp6-rtc-offset-sync = {
      wantedBy = [ "multi-user.target" ];
      pathConfig = { PathChanged = "/run/systemd/timesync/synchronized"; Unit = "fp6-rtc-offset-save.service"; };
    };
  };
}
