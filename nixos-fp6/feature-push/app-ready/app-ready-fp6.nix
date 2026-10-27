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
#    300 > shell 200; kept ready 600; background 800/900), PSI-triggered
#    low-memory killer (background apps, unpinned first, never the focused
#    app or the shell), hidden prelaunch at login, page-cache preload
#    (fadvise WILLNEED on the app's learned mapped files / package files),
#    zram on/off, status JSON for the page; D-Bus com.fairphone6.AppReady.
#  * zram swap (zstd, 50 % of RAM, swappiness 100, page-cluster 0) and
#    earlyoom as the root backstop (avoids the shell and core services).
#
# Defaults: zram on, low-memory killer on, nothing kept ready / prelaunched /
# preloaded until the user picks apps.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.appReady;
  package = pkgs.callPackage ./package.nix { inherit (pkgs.lomiri) lomiri-app-launch; };
  schema = pkgs.callPackage ./schema.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-app-ready.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
  # earlyoom matches /proc/<pid>/comm (15 chars): Lomiri is ".lomiri-wrapped",
  # Maliit "..maliit-keyboa"/"maliit-server". No backslashes: "[.]" for a dot.
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
    zram = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "zram swap device (the Settings switch can still turn it off at runtime).";
      };
      memoryPercent = lib.mkOption { type = lib.types.int; default = 50; };
      swappiness = lib.mkOption { type = lib.types.int; default = 100; };
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
    }) cfg.extraPreload;

    nixpkgs.overlays = lib.mkOrder 2600 [ (_final: prev: {
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
    }) ];

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
      "vm.swappiness" = lib.mkDefault cfg.zram.swappiness;
      # zram: no readahead clustering of swap-ins.
      "vm.page-cluster" = lib.mkDefault 0;
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
    security.polkit.extraConfig = lib.mkIf cfg.zram.enable ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.freedesktop.systemd1.manage-units" &&
            subject.user == "${user}" &&
            (action.lookup("unit") == "fp6-zram-on.service" ||
             action.lookup("unit") == "fp6-zram-off.service") &&
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
      "${lib.getExe' config.services.earlyoom.package "earlyoom"} -m 4 -s 10 -r 3600 --avoid \"${lib.replaceStrings [ "$" ] [ "$$" ] avoid}\""
    ]);

    assertions = [{
      assertion = config.services.fp6LomiriSensors.enable or false;
      message = "fp6 app-ready runs in the Lomiri session of services.fp6LomiriSensors.user.";
    }];
  };
}
