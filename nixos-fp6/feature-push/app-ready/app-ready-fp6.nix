# App launch / keep apps ready (feature push 2026-10-24): make chosen apps
# open fast, like phones that keep apps loaded. Phone-level setting in
# System Settings > App Launch; no app is rebuilt.
# Notes, research and measurements: results/feature-push-20261024/app-ready/README.md.
#
#  * Lomiri patch (lomiri-app-ready.patch, overlay lib.mkOrder 2600, after
#    drawer-folders 2500): nixpkgs' Lomiri never suspends apps (Debian patch
#    2012, no media-hub). Stage.qml now suspends (SIGSTOP via
#    lomiri-app-launch) only the apps the user keeps ready, or every
#    background app if "freeze-background" is on. Hidden launch: a marker in
#    $XDG_RUNTIME_DIR/fp6-app-ready/background/<appid> makes the app's first
#    window open unfocused at the bottom of the stack and off screen.
#    Lomiri writes the focused app id to $XDG_RUNTIME_DIR/fp6-app-ready/focused
#    and logs "fp6-app-ready: added / first-frame / focus" lines.
#    Links into a kept-ready app: lomiri-app-launch's second start of a
#    running app (url-dispatcher, another app) is LomiriResumeRequest ->
#    Open(urls) -> LomiriFocusRequest. A frozen app cannot answer Open, and
#    qtmir's focus request was ignored for a still-background (prelaunched)
#    window, so the link waited for a tap. The patched TopLevelWindowModel
#    activates the background window on either request (which resumes it);
#    fp6-app-ready SIGCONTs an app still fully stopped 1.5 s after a resume
#    request as the fallback ("still frozen ... after a start request").
#  * fp6-app-ready (Rust, user service): oom_score_adj by state (foreground
#    300 > session services 200 > shell/OSK 100; kept ready 600; background
#    800/900), PSI-triggered low-memory killer (background lomiri-app-launch
#    apps, unpinned first, never the focused app, never a process at
#    oom_score_adj <= 200, never lomiri.service / lomiri-keyboard.service:
#    crash-isolation's interface), hidden prelaunch at login, page-cache preload
#    (fadvise WILLNEED on the app's learned mapped files / package files),
#    zram on/off, status JSON for the page; D-Bus com.fairphone6.AppReady.
#  * zram swap (zstd, size by memory profile, page-cluster 0) and
#    earlyoom as the root backstop (never kills the shell or Maliit:
#    --ignore; avoids core services).
#  * Memory & performance (2026-10-28, results/feature-push-20261024/
#    memory-performance/README.md): on-device usage prediction keeps the
#    likely next apps ready (GSettings "predicted", suspended by Stage.qml:
#    lomiri-memory-predicted.patch, overlay lib.mkOrder 4000), starts them
#    hidden and warms their files; the shell's files are warmed after login
#    and after a reclaim; profiles Conservative / Balanced / Aggressive set
#    the daemon's numbers and, as root (fp6-memory-profile@<name>.service,
#    `fp6-app-ready apply-profile`), vm sysctls, read-ahead and MGLRU; the
#    zram size follows the profile (fp6-zram-size@<pct>.service).
#
# Defaults: zram on, low-memory killer on, profile Aggressive with
# prediction on; nothing in the user's always-keep / prelaunch / preload
# lists until the user picks apps.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.appReady;
  package = pkgs.callPackage ./package.nix { inherit (pkgs.lomiri) lomiri-app-launch; };
  schema = pkgs.callPackage ./schema.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-app-ready.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
  # zram size per profile; profile.rs checks it says the same.
  zramPercentFor = builtins.fromJSON (builtins.readFile ./fp6-app-ready/zram-percent.json);
  profiles = [ "conservative" "balanced" "aggressive" ];
  # earlyoom matches /proc/<pid>/comm (15 chars): Lomiri is ".lomiri-wrapped",
  # Maliit "..maliit-keyboa"/"maliit-server". No backslashes: "[.]" for a dot.
  # Shell (lomiri.service, also the greeter) and OSK (lomiri-keyboard.service)
  # are never killed (crash-isolation interface); --avoid only lowers a
  # process's rank, --ignore excludes it.
  ignore = "^([.]lomiri-wrapped|lomiri|[.]{0,2}maliit.*|maliit-server)$";
  avoid = "^([.]lomiri-wrapped|lomiri|[.]{0,2}maliit.*|maliit-server|systemd|systemd-.*|[(]sd-pam[)]|dbus-daemon|dbus-broker.*|pipewire.*|wireplumber|lightdm|sshd.*|ofonod|NetworkManager|ModemManager|rmtfs|pd-mapper|tqftpserv|qrtr-ns|fp6-.*|[.]fp6-.*|hfd-service|repowerd|biometryd|lomiri-app-laun.*|Xwayland)$";
in {
  options.hardware.fairphone6.appReady = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Install fp6-app-ready, the Lomiri lifecycle/background-launch patch,
        zram swap, earlyoom and Settings > App Launch. Keep-ready, prelaunch
        and preload lists start empty; the user picks apps in Settings.
      '';
    };
    memoryProfile = lib.mkOption {
      type = lib.types.enum profiles;
      default = "aggressive";
      description = ''
        Memory profile applied at boot (vm sysctls, read-ahead, zram size)
        until the user's Settings > Memory & performance choice is applied
        at login (GSettings com.fairphone6.app-ready profile, default
        aggressive too).
      '';
    };
    zram = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "zram swap device (the Settings switch can still turn it off at runtime).";
      };
      memoryPercent = lib.mkOption {
        type = lib.types.int;
        default = zramPercentFor.${cfg.memoryProfile};
        defaultText = "the boot profile's size (25 / 50 / 75 %)";
      };
    };
    appQmlCache = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        QML disk cache for the apps (QML_FORCE_DISK_CACHE=1 in the systemd
        user manager's DefaultEnvironment): nixpkgs' Qt 5 compiles every QML
        file again on each start without it (System Settings, Messages,
        Phone, Gallery had no cache on the phone, 2026-10-28). fp6-app-ready
        empties the app caches once after a system change (qmlcache.rs).
      '';
    };
    loopDirectIo = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Experimental, needs a supervised test: switch the loop devices under
        "/" to direct I/O at boot, so file pages of the root image are cached
        once instead of up to three times (root file system, the image file
        on the outer file system, the outer partition's block cache). On the
        phone (2026-10-28) "Buffers" was 2.3 GiB next to 3.0 GiB "Cached"
        20 minutes after boot.
      '';
    };
    earlyoom = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "earlyoom backstop below fp6-app-ready's own killer (4 % RAM and 10 % swap free).";
    };
    extraPreload = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.str);
      default = { };
      example = { lomiri-camera-app = [ "/run/current-system/sw/share/libcamera" ]; };
      description = ''
        Extra files or directories preloaded with an app (by app id), as
        /etc/fp6-app-ready/preload.d/<appid>.list. Only used when the app is
        in the user's preload list.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ package ];
    system.build.fp6-app-ready = { inherit package lssPlugin schema; inherit (pkgs.lomiri) lomiri qtmir; };

    environment.etc = lib.mapAttrs' (app: paths: lib.nameValuePair "fp6-app-ready/preload.d/${app}.list" {
      text = lib.concatMapStrings (p: "${p}\n") paths;
    }) cfg.extraPreload // lib.optionalAttrs cfg.appQmlCache {
      # App QML disk caches (see appQmlCache). A drop-in of its own:
      # lomiri-polish sets systemd.user.settings.Manager.DefaultEnvironment,
      # and DefaultEnvironment= lines add up across user.conf.d files.
      "systemd/user.conf.d/fp6-app-qmlcache.conf".text = ''
        [Manager]
        DefaultEnvironment=QML_FORCE_DISK_CACHE=1
      '';
    };

    nixpkgs.overlays = lib.mkMerge [ (lib.mkOrder 2600 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-app-ready.patch ];
          # Stage.qml reads com.fairphone6.app-ready: wrapGAppsHook adds the schema dir.
          buildInputs = (old.buildInputs or [ ]) ++ [ schema ];
          passthru = (old.passthru or { }) // { fp6AppReady = true; };
        });
        # Morph's surfaceless QtWebEngine connection kept it from ever being
        # suspended (stuck in SuspendingWaitSession, seen live 2026-10-27).
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./qtmir-suspend-surfaceless-sessions.patch ];
        });
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
        });
      });
    }) ])
    # Memory & performance: predicted apps suspended like keep-ready ones,
    # never-keep apps not, hidden-started windows not in the app switcher.
    # In postPatch, after spread-close-all's (mkOrder 3300, also postPatch):
    # its card-opacity line is in this patch's context.
    (lib.mkOrder 4000 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          postPatch = (old.postPatch or "") + ''
            patch -p1 --no-backup-if-mismatch < ${./lomiri-memory-predicted.patch}
          '';
          passthru = (old.passthru or { }) // { fp6MemoryPredicted = true; };
        });
      });
    }) ])
    # lomiri-app-launch aborted on exit (CLI, fp6-app-ready's prelaunch):
    # its jobFailed D-Bus signal callback threw once the registry was gone
    # (std::terminate), dereferenced null UnitData that unitPath() had
    # inserted, and the registry's GLib thread kept dispatching during the
    # static destructors. The patch catches and logs in the callback, uses
    # find() in unitPath(), drops the default registry from an atexit
    # handler (joins the thread first) and sets the CLI's result only once.
    (lib.mkOrder 4400 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-app-launch = lprev.lomiri-app-launch.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-app-launch-exit-and-signal-safety.patch ];
          passthru = (old.passthru or { }) // { fp6ExitSafety = true; };
        });
      });
    }) ]) ];

    systemd.user.services.fp6-app-ready = {
      description = "FP6 keep apps ready (OOM priorities, low-memory killer, prelaunch, preload)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      # Not for the LightDM greeter's user manager.
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = "${lib.getExe package} run";
        Restart = "on-failure";
        RestartSec = 5;
        # Prelaunched apps are lomiri-app-launch units of their own, not children.
        KillMode = "process";
        Nice = 5;
        IOSchedulingClass = "idle";
        # The killer must survive the pressure it handles.
        OOMScoreAdjust = 200;
      };
    };

    zramSwap = lib.mkIf cfg.zram.enable {
      enable = true;
      algorithm = "zstd";
      memoryPercent = cfg.zram.memoryPercent;
      priority = 100;
    };
    boot.kernel.sysctl = lib.mkIf cfg.zram.enable {
      # zram: no readahead clustering of swap-ins. Swappiness, cache pressure
      # and watermarks come from the profile (fp6-memory-profile@).
      "vm.page-cluster" = lib.mkDefault 0;
    };

    # Root side of a memory profile: vm sysctls, read-ahead of the root
    # device chain, MGLRU when the kernel has it. At boot with the configured
    # profile, at login / on a Settings change with the user's (fp6-app-ready
    # starts the instance; polkit below).
    systemd.services."fp6-memory-profile@" = {
      description = "Memory profile %i (Settings > Memory & performance)";
      after = [ "systemd-sysctl.service" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${package}/bin/fp6-app-ready apply-profile %i";
      };
    };
    systemd.services.fp6-memory-profile-boot = {
      description = "Memory profile at boot";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-sysctl.service" "local-fs.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${package}/bin/fp6-app-ready apply-profile ${cfg.memoryProfile}";
      };
      path = [ pkgs.util-linux ];
      # Experimental (loopDirectIo): one copy of the root image's pages in the cache.
      postStart = lib.optionalString cfg.loopDirectIo ''
        for dev in /sys/block/loop*; do
          [ -e "$dev/loop/backing_file" ] || continue
          losetup --direct-io=on "/dev/$(basename "$dev")" || true
          echo "$(basename "$dev") dio=$(cat "$dev/loop/dio")"
        done
      '';
    };

    # zram size from Settings (25/50/75/100 % of RAM). Not through
    # systemd-zram-setup@zram0 (see fp6-zram-off). Refuses when the data now
    # in zram would not fit back into available RAM: the size then applies
    # at the next login, when zram is nearly empty.
    systemd.services."fp6-zram-size@" = lib.mkIf cfg.zram.enable {
      description = "Compressed swap size %i % of RAM (Settings > Memory & performance)";
      serviceConfig = { Type = "oneshot"; TimeoutStartSec = 300; };
      path = [ pkgs.util-linux pkgs.gnugrep pkgs.coreutils pkgs.gawk ];
      scriptArgs = "%i";
      script = ''
        pct=$1
        case $pct in 25|50|75|100) ;; *) echo "bad size $pct"; exit 1 ;; esac
        z=/sys/block/zram0
        [ -e $z/disksize ] || exit 0
        total=$(awk '/^MemTotal:/ {print $2 * 1024}' /proc/meminfo)
        avail=$(awk '/^MemAvailable:/ {print $2 * 1024}' /proc/meminfo)
        want=$(( total / 100 * pct ))
        have=$(cat $z/disksize)
        stored=$(awk '{print $1}' $z/mm_stat)
        if [ "$have" -gt 0 ] && [ $(( (want - have) / (total / 100) )) -ge -2 ] && [ $(( (want - have) / (total / 100) )) -le 2 ]; then
          echo "zram already $have bytes"; exit 0
        fi
        if [ "''${stored:-0}" -gt $(( avail / 2 )) ]; then
          echo "zram holds $stored bytes, $avail available: not now"; exit 0
        fi
        if grep -q '^/dev/zram0 ' /proc/swaps; then swapoff /dev/zram0; fi
        echo 1 > $z/reset
        echo zstd > $z/comp_algorithm
        echo "$want" > $z/disksize
        mkswap /dev/zram0 >/dev/null
        swapon -p 100 /dev/zram0
        echo "zram now $want bytes ($pct % of RAM)"
      '';
    };

    # Settings > App Launch zram switch. Not by stopping dev-zram0.swap: its
    # BindsTo'd systemd-zram-setup@zram0 then resets the device right after
    # swapoff, which fails with EBUSY (udev still probes it) and leaves the
    # setup unit failed, so a later start cannot configure the device again
    # (seen live 2026-10-27). These two units only swapoff / swapon the
    # configured device; fp6-app-ready (the phone user's service, not in a
    # logind session) may start exactly these two.
    systemd.services.fp6-zram-off = lib.mkIf cfg.zram.enable {
      description = "Compressed swap off (Settings > App Launch)";
      # swapoff reads every swapped page back into RAM.
      serviceConfig = { Type = "oneshot"; TimeoutStartSec = 300; };
      path = [ pkgs.util-linux pkgs.gnugrep ];
      script = ''
        if grep -q '^/dev/zram0 ' /proc/swaps; then swapoff /dev/zram0; fi
      '';
    };
    systemd.services.fp6-zram-on = lib.mkIf cfg.zram.enable {
      description = "Compressed swap on (Settings > App Launch)";
      serviceConfig.Type = "oneshot";
      path = [ pkgs.util-linux pkgs.gnugrep pkgs.coreutils config.systemd.package ];
      script = ''
        grep -q '^/dev/zram0 ' /proc/swaps && exit 0
        if [ "$(cat /sys/block/zram0/disksize)" != 0 ]; then
          swapon -p 100 /dev/zram0
        else
          systemctl reset-failed systemd-zram-setup@zram0.service || true
          systemctl start dev-zram0.swap
        fi
      '';
    };
    security.polkit.extraConfig = lib.optionalString cfg.zram.enable ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${user}" &&
            (action.lookup("unit") == "fp6-zram-on.service" ||
             action.lookup("unit") == "fp6-zram-off.service" ||
             ${lib.concatMapStringsSep " ||\n             " (p: ''action.lookup("unit") == "fp6-zram-size@${toString p}.service"'') [ 25 50 75 100 ]}) &&
            action.lookup("verb") == "start") {
          return polkit.Result.YES;
        }
      });
    '' + ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${user}" &&
            (${lib.concatMapStringsSep " ||\n             " (p: ''action.lookup("unit") == "fp6-memory-profile@${p}.service"'') profiles}) &&
            action.lookup("verb") == "start") {
          return polkit.Result.YES;
        }
      });
    '';

    services.earlyoom = lib.mkIf cfg.earlyoom {
      enable = true;
      freeMemThreshold = 4;
      freeSwapThreshold = 10;
      enableNotifications = false;
    };
    # The module passes extraArgs through $EARLYOOM_ARGS, which systemd splits
    # at whitespace without honouring the shell quotes: a regex would arrive
    # with literal quote characters. Own ExecStart instead ("$$" = "$").
    systemd.services.earlyoom.serviceConfig.ExecStart = lib.mkIf cfg.earlyoom (lib.mkForce [
      ""
      "${lib.getExe' config.services.earlyoom.package "earlyoom"} -m 4 -s 10 -r 3600 --ignore \"${lib.replaceStrings [ "$" ] [ "$$" ] ignore}\" --avoid \"${lib.replaceStrings [ "$" ] [ "$$" ] avoid}\""
    ]);

    assertions = [{
      assertion = config.services.fp6LomiriSensors.enable or false;
      message = "fp6 app-ready runs in the Lomiri session of services.fp6LomiriSensors.user.";
    }];
  };
}
