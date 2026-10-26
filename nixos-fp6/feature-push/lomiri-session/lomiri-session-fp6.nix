# Lomiri session life cycle: greeter exit at login, user session end at
# display-manager stop. Notes and measurements:
# results/feature-push-20261024/lomiri-session/README.md.
#
#  1. Greeter Lomiri hung for 90 s after every login (session-cN.scope "Stopping
#     timed out", ~350 MB held while the user session starts). SIGTERM was
#     handled (Mir terminator -> QCoreApplication::quit), the QML scene was torn
#     down, then the main thread deadlocked in ~QGuiApplicationPrivate:
#     QCoreApplicationPrivate::cleanupThreadData holds the post-event-list mutex
#     while deleting undelivered events; one was a biometryd QML
#     DispatcherWithContext Task (the greeter's fingerprint identification)
#     holding the last reference to its observer, whose destructor calls
#     dispatcher->finalize() -> QCoreApplication::postEvent -> same mutex.
#     biometryd-qml-no-post-at-exit.patch: no posting once the application is
#     closing down (the dispatcher is leaked; the process is exiting).
#     Safety net (greeterStopTimeout): the LightDM greeter wrapper runs the
#     upstream lomiri-greeter-wrapper as a child and, on SIGTERM, SIGKILLs this
#     session's remaining greeter processes after a few seconds instead of
#     logind's 90 s scope stop timeout. Greeter-only; user sessions untouched.
#  2. The user's lomiri.service survived a display-manager restart: headless
#     Lomiri (~350 MB) until the next login stopped it. lomiri-session's
#     run-systemd-session waited in a foreground `systemctl --user start --wait`
#     with no signal handler, and dm-lomiri-session (exec'd by the NixOS session
#     wrapper, so the one process LightDM's session child signals) didn't pass
#     SIGTERM on. lomiri-session-session-end.patch: dm-lomiri-session forwards
#     TERM/HUP/INT, run-systemd-session traps them and stops lomiri.service +
#     graphical-session(-pre).target (--no-block) in the user manager.
#     Same patch, login time: run-systemd-session's prelude took ~3.5 s (one
#     `systemctl show` per user unit file plus an unconditional daemon-reload);
#     now one `show` for all units and a reload only when its
#     $XDG_RUNTIME_DIR drop-ins changed (first login of a user manager).
#
# Overlay order: lib.mkOrder 2300, after lomiri-polish (2000), osk-wayland
# (2100) and lomiri-multiwindow (2200). Only configuration.nix also patches
# lomiri-session (lomiri-session.in); this patch touches other files.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriSession;
  bin = lib.getBin;
  greeterStop = pkgs.writeShellScript "fp6-lomiri-greeter-stop" ''
    # $1 = seconds to wait; kills (TERM, then KILL) every process of the calling
    # login session's scope that belongs to this user, except the caller ($2).
    timeout_ds=$(( $1 * 10 ))
    read -r cgline < /proc/self/cgroup
    procs=/sys/fs/cgroup''${cgline#0::}/cgroup.procs
    pids=()
    while read -r p; do
      [ "$p" = "$2" ] || [ "$p" = "$$" ] && continue
      [ -O "/proc/$p" ] && pids+=("$p")
    done < "$procs"
    [ ''${#pids[@]} -gt 0 ] || exit 0
    kill -TERM "''${pids[@]}" 2>/dev/null
    alive() {
      local k v
      while read -r k v _; do
        [ "$k" = State: ] && { [ "$v" != Z ]; return; }
      done < "/proc/$1/status" 2>/dev/null
      return 1
    }
    for ((i = 0; i < timeout_ds; i++)); do
      left=0
      for p in "''${pids[@]}"; do alive "$p" && left=1; done
      [ $left = 0 ] && exit 0
      ${bin pkgs.coreutils}/bin/sleep 0.1
    done
    for p in "''${pids[@]}"; do
      alive "$p" && echo "fp6-lomiri-greeter: pid $p still running after $1 s, sending SIGKILL" >&2
    done
    kill -KILL "''${pids[@]}" 2>/dev/null
    exit 0
  '';
in {
  options.hardware.fairphone6.lomiriSession = {
    enable = lib.mkEnableOption ''
      the Lomiri session life-cycle fixes (greeter exits at login, user
      session units stop when the LightDM session ends)'';
    greeterStopTimeout = lib.mkOption {
      type = lib.types.ints.between 0 30;
      default = 3;
      description = ''
        Seconds the LightDM greeter wrapper waits after SIGTERM for the greeter
        Lomiri to exit before it SIGKILLs it (0 disables the supervision and
        restores the plain `exec lomiri-greeter-wrapper`).
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 2300 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        biometryd = lprev.biometryd.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./biometryd-qml-no-post-at-exit.patch ];
          passthru = (old.passthru or { }) // { fp6NoPostAtExit = true; };
        });
        lomiri-session = lprev.lomiri-session.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-session-session-end.patch ];
          passthru = (old.passthru or { }) // { fp6StopOnSessionEnd = true; };
        });
      });
    }) ];

    # configuration.nix's greeter wrapper runs these lines and then
    # `exec lomiri-greeter-wrapper "$@"`; with the supervision on, the lines
    # below run the upstream wrapper themselves and never return. mkAfter: after
    # the QML-cache exports of boot-speed.
    hardware.fairphone6.lomiriGreeterWrapperPre = lib.mkIf (cfg.greeterStopTimeout > 0) (lib.mkAfter ''
      # fp6 lomiri-session: supervise the greeter so that it can't outlive a login.
      ${lib.getExe' pkgs.lomiri.lomiri "lomiri-greeter-wrapper"} "$@" &
      fp6_greeter_child=$!
      trap '${greeterStop} ${toString cfg.greeterStopTimeout} $$; exit 0' TERM INT HUP
      wait "$fp6_greeter_child"
      fp6_greeter_rc=$?
      trap - TERM INT HUP
      exit "$fp6_greeter_rc"
    '');

    system.build.fp6-lomiri-session = pkgs.lomiri.lomiri-session;
    system.build.fp6-biometryd = pkgs.lomiri.biometryd;
    system.build.fp6-lomiri-greeter-stop = greeterStop;
  };
}
