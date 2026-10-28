# Crash isolation (feature push, 2026-10-28): an app crash must close the app,
# not the phone session. Diagnosis, interface and checklist:
# results/feature-push-20261024/crash-isolation/README.md.
#
#  1. Root cause of the observed "app crash logs me out" (2026-10-28 11:06:42):
#     Lomiri itself died of SIGSEGV on the Mir/Wayland thread, in libwayland's
#     serialize_closure -> memcpy of an *array* argument. The Mir 2.15
#     input-method-v1 backport (nixos-fp6/mir-input-method-v1.patch) stored
#     the wl_array* of the input method's modifiers_map request; libwayland
#     frees it when the handler returns, and the next text change (an OSK key
#     press) forwarded the freed memory to the text-input-v1/v2 app. Maliit
#     sends modifiers_map on every activation, so every focused text field in
#     a text-input v1/v2 client (Qt 6 apps such as Morph) was exposed.
#     mir-input-method-v1-own-modifiers-map.patch keeps a copy of the bytes
#     (upstream Mir does the same with CopyableWlArray). Overlay mkOrder 4100,
#     after configuration.nix's mir_2_15 patch.
#  2. A shell crash was a logout: lomiri.service had no Restart=, and
#     run-systemd-session's `systemctl --user start --wait lomiri.service`
#     returns at the first failure, which ended the LightDM session.
#     (`lal-application-end.target not found` in ExecStopPost is harmless.)
#     Now lomiri.service restarts on failure (start limit: 4 starts in 5 min,
#     i.e. 3 crashes; then the session ends as before) and
#     lomiri-session-shell-restart.patch keeps the session while it does. The
#     restarted shell starts LOCKED (--mode=full-greeter: the same integrated
#     greeter as the normal lock screen), via LOMIRI_BINARY and a marker
#     written by ExecStopPost. Open apps are lost (Wayland clients cannot
#     survive their compositor); session services (indicators, app-ready,
#     telephony) keep running - graphical-session.target stays up.
#  3. OOM order: shell + OSK at oom_score_adj 100 (OOMScoreAdjust=, the lowest
#     an unprivileged user unit may set: user@.service runs at 100), session
#     services 200 (user manager default), apps >= 300 (app-ready).
#  4. Respawning helpers (lomiri-kdeconnect restarted 548 times in 48 min,
#     each start a new Wayland client): user services get exponential restart
#     back-off (RestartSteps / RestartMaxDelaySec, a type-level service.d
#     drop-in); the shell and the OSK keep their fixed delay.
#  5. fp6-crash-notify (Rust user service): "<App> stopped" when a
#     lomiri-app-launch app (or the shell) dies of a crash signal, tap =
#     Report (starts fp6-bugreport.service). SIGKILL/SIGTERM/exit codes
#     (app-ready's killer, the OOM killer, closing an app) are not crashes.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.crashIsolation;
  user = config.services.fp6LomiriSensors.user or "user";
  bugreport = config.hardware.fairphone6.bugreport.enable or false;
  marker = "fp6-crash-isolation/shell-crashed";

  notifier = pkgs.callPackage ./package.nix { };

  # ExecStopPost of lomiri.service: remember a crash for the restart.
  shellStopPost = pkgs.writeShellScript "fp6-lomiri-crash-mark" ''
    case "''${SERVICE_RESULT:-success}" in
      success) exit 0 ;;
    esac
    dir="''${XDG_RUNTIME_DIR:-/run/user/$(${pkgs.coreutils}/bin/id -u)}/fp6-crash-isolation"
    ${pkgs.coreutils}/bin/mkdir -p "$dir"
    echo "$(${pkgs.coreutils}/bin/date +%s) $SERVICE_RESULT ''${EXIT_CODE:-} ''${EXIT_STATUS:-}" > "$dir/shell-crashed"
    echo "fp6-crash-isolation: shell ended with $SERVICE_RESULT (''${EXIT_CODE:-}/''${EXIT_STATUS:-}); a restart starts locked" >&2
  '';

  # LOMIRI_BINARY for lomiri-session (`exec ''${LOMIRI_BINARY:-lomiri} --mode=$MODE "$@"`):
  # after a crash, start with the lock screen (Shell.qml: forcedUnlock only
  # for full-shell). PATH is lomiri-session's, so `lomiri` is the same binary.
  shellLauncher = pkgs.writeShellScript "fp6-lomiri-launch" ''
    m="''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/${marker}"
    if [ -e "$m" ]; then
      echo "fp6-crash-isolation: shell restarted after a crash ($(cat "$m" 2>/dev/null)); starting locked" >&2
      rm -f "$m"
      args=()
      for a in "$@"; do
        [ "$a" = --mode=full-shell ] && a=--mode=full-greeter
        args+=("$a")
      done
      set -- "''${args[@]}"
    fi
    exec lomiri "$@"
  '';

  # lomiri.service: a drop-in of our own, not systemd.user.services.lomiri
  # (that would also set NixOS's minimal PATH= and hide the session's PATH
  # from lomiri-session, which needs gdbus, Xwayland, ...).
  shellDropin = ''
    # fp6 crash-isolation (crash-isolation-fp6.nix)
    [Unit]
  '' + lib.optionalString cfg.shellRestart ''
    # 4 starts in 5 min: the login plus 3 crash restarts; a 4th crash ends the
    # session (the next login resets the limit: lomiri-session-shell-restart.patch).
    StartLimitIntervalSec=300
    StartLimitBurst=4
  '' + ''
    [Service]
    # OOM: shell 100 < session services 200 < apps >= 300 (app-ready).
    OOMScoreAdjust=100
    # No restart back-off for the shell (service.d/10-fp6-restart-backoff.conf).
    RestartSteps=0
  '' + lib.optionalString cfg.shellRestart ''
    Restart=on-failure
    RestartSec=1
    ExecStopPost=${shellStopPost}
    Environment=LOMIRI_BINARY=${shellLauncher}
  '';

  # Type-level drop-in for every user service, and the exemptions (unit
  # drop-ins sort after it: "zz-").
  dropins = pkgs.runCommand "fp6-crash-isolation-user-dropins" { } (''
    d=$out/lib/systemd/user
    mkdir -p $d/lomiri-keyboard.service.d $d/lomiri.service.d
    cp ${pkgs.writeText "lomiri-crash-isolation.conf" shellDropin} $d/lomiri.service.d/zz-fp6-crash-isolation.conf
  '' + lib.optionalString cfg.restartBackoff.enable ''
    mkdir -p $d/service.d
    cat > $d/service.d/10-fp6-restart-backoff.conf <<'EOF'
    # fp6 crash-isolation: a helper that keeps failing is restarted ever more
    # slowly (RestartSec -> ${toString cfg.restartBackoff.maxDelaySec} s in ${toString cfg.restartBackoff.steps} steps)
    # instead of every few seconds forever. Only units with Restart= are affected.
    [Service]
    RestartSteps=${toString cfg.restartBackoff.steps}
    RestartMaxDelaySec=${toString cfg.restartBackoff.maxDelaySec}
    EOF
  '' + ''
    cat > $d/lomiri-keyboard.service.d/zz-fp6-crash-isolation.conf <<'EOF'
    # fp6 crash-isolation: the OSK comes back with the shell, without back-off,
    # and is OOM-protected like the shell.
    [Service]
    RestartSteps=0
    OOMScoreAdjust=100
    EOF
  '');
in {
  options.hardware.fairphone6.crashIsolation = {
    enable = lib.mkEnableOption "crash isolation: app crashes close the app, a shell crash restarts the shell (locked) instead of logging out";
    shellRestart = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Restart a crashed Lomiri in the same session, locked, instead of ending the session.";
    };
    notify = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "fp6-crash-notify: \"<App> stopped\" notifications with a Report action.";
    };
    restartBackoff = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Exponential restart back-off for user services (not the shell or the OSK).";
      };
      steps = lib.mkOption { type = lib.types.ints.between 1 20; default = 6; description = "Restart delay steps from RestartSec= to maxDelaySec."; };
      maxDelaySec = lib.mkOption { type = lib.types.ints.between 5 3600; default = 120; description = "Longest restart delay of a repeatedly failing user service."; };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      # 1. The Mir crash itself.
      nixpkgs.overlays = lib.mkOrder 4100 [ (_final: prev: {
        mir_2_15 =
          assert lib.assertMsg (lib.any (p: lib.hasSuffix "mir-input-method-v1.patch" (toString p)) (prev.mir_2_15.patches or [ ]))
            "crash-isolation: mir-input-method-v1-own-modifiers-map.patch applies on top of nixos-fp6/mir-input-method-v1.patch";
          prev.mir_2_15.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./mir-input-method-v1-own-modifiers-map.patch ];
            passthru = (old.passthru or { }) // { fp6OwnModifiersMap = true; };
          });
      }) ];
      system.build.fp6-mir-2-15 = pkgs.mir_2_15;

      # 3. OOM order (README, interface) and 4. back-off: user drop-ins.
      systemd.packages = [ dropins ];   # also provides user units (lib/systemd/user)
      system.build.fp6-crash-isolation-dropins = dropins;
    }


    # 2. Shell restart without logout.
    (lib.mkIf cfg.shellRestart {
      # lomiri.service settings: shellDropin above.
      nixpkgs.overlays = lib.mkOrder 4100 [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri-session = lprev.lomiri-session.overrideAttrs (old: {
            patches = (old.patches or [ ]) ++ [ ./lomiri-session-shell-restart.patch ];
            passthru = (old.passthru or { }) // { fp6ShellRestart = true; };
          });
        });
      }) ];
      system.build.fp6-lomiri-crash-mark = shellStopPost;
      system.build.fp6-lomiri-launch = shellLauncher;
      assertions = [{
        assertion = config.hardware.fairphone6.lomiriSession.enable or false;
        message = "crash-isolation's lomiri-session-shell-restart.patch applies on top of lomiri-session/lomiri-session-session-end.patch (hardware.fairphone6.lomiriSession.enable).";
      }];
    })

    # 5. Notifications.
    (lib.mkIf cfg.notify {
      systemd.user.services.fp6-crash-notify = {
        description = "FP6 crash notifications (app stopped / shell restarted, Report)";
        wantedBy = [ "graphical-session.target" ];
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        unitConfig.ConditionUser = user;
        serviceConfig = {
          ExecStart = lib.escapeShellArgs ([ (lib.getExe notifier) ]
            ++ lib.optionals bugreport [ "--report-unit" "fp6-bugreport.service" ]);
          Restart = "on-failure";
          RestartSec = 5;
          Nice = 5;
        };
      };
      system.build.fp6-crash-notify = notifier;
    })
  ]);
}
