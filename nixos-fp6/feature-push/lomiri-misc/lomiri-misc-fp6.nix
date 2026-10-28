# Lomiri misc fixes for the FP6 (2026-10-24). No-op unless `enable` is set;
# each part has its own switch.
#
#  * launcherPins: Lomiri's launcher reads its default pins from the GSettings
#    key com.lomiri.Shell.Launcher `items` (lomiri-schemas). The stock default
#    names Ubuntu Touch click packages (appid://openstore.openstore-team/...,
#    appid://camera.ubports/..., appid://gallery.ubports/...) that do not
#    exist on NixOS, so Camera, Gallery and OpenStore pins were dropped. The
#    new default is a dconf *system* database value (/etc/dconf/profile/user
#    already lists file-dbs; Lomiri's wrapper ships the dconf GIO module), so
#    nothing is rebuilt and a user's own choice (user-db) still wins.
#    A .gschema.override in the system profile would not be seen: Lomiri's
#    wrapper *prefixes* XDG_DATA_DIRS with lomiri-schemas' own schema dir.
#    AccountsService com.lomiri.shell.AccountsService LauncherItems is only a
#    mirror Lomiri writes for the greeter; it is not the source of the pins.
#  * greeterKeyboard: the greeter's lomiri-keyboard.service (user@78) is
#    started by lomiri-greeter-wrapper, but on login session-c1.scope is
#    SIGTERMed and the wrapper's TERM trap exits before its
#    `systemctl --user stop lomiri-keyboard.service`. Maliit then loses the
#    greeter's Mir connection (exit 1), Restart=on-failure restarts it twice
#    while user@78 lingers ~10 s, and each restart qFatal()s/SIGABRTs in
#    QGuiApplicationPrivate::init ("Failed to create wl_display", no QPA
#    platform) because the greeter's wayland-0 socket is gone. ExecCondition
#    now waits (up to 5 s) for the compositor socket and otherwise skips the
#    start cleanly (condition failure: no restart, no core dump). In a live
#    session (greeter or user) the socket exists and the check passes at once,
#    so ExecStart / ExecStopPost / Restart / PartOf stay as lomiri-gpu-fp6.nix
#    and lomiri-polish-fp6.nix set them.
#  * hideIndicatorsClient: Lomiri installs indicators-client.desktop, its
#    developer test app for the indicator menus, as a visible touch app with
#    an empty Icon= (a blank tile in the app drawer; fp6-selftest's desktops
#    check failed on it on r4). It is set to NoDisplay=true; the binary stays
#    installed.
#  * alarmsMkcal: Lomiri UITK's Alarm API (the Clock app, the Assistant) uses
#    the QtOrganizer manager named by ALARM_BACKEND, default "eds". There is
#    no QtOrganizer EDS plugin on NixOS, so UITK fell back to "memory" plus a
#    private ~/.local/share/<app>/alarms.json: the Clock app saw its own
#    alarms, but indicator-datetime (built with ENABLE_MKCAL; it is what
#    rings alarms, from the mkcal notebook "Alarms") never did. The session
#    now sets ALARM_BACKEND=mkcal and puts only the qtorganizer-mkcal plugin
#    on QT_PLUGIN_PATH (its own plugin root in the system profile), the store
#    lomiri-calendar-app and indicator-datetime already use. Alarms made
#    before the change stay in that app's alarms.json (set them again).
#    Laptop test: nix-build lomiri-misc/alarms-mkcal-test.nix.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriMisc;

  launcherItems = map (id: "application:///${id}.desktop") cfg.launcherPins;

  waitWaylandSocket = pkgs.writeShellScript "fp6-lomiri-keyboard-wait-wayland" ''
    rt="''${XDG_RUNTIME_DIR:-/run/user/$(${pkgs.coreutils}/bin/id -u)}"
    # lomiri-greeter-wrapper calls `set_greeter_var WAYLAND_DISPLAY=wayland-0`
    # (one argument), which exports "wayland-0=", and Mir then creates its
    # socket under that literal name ($rt/wayland-0=, seen 2026-10-26). Accept
    # that socket and link wayland-0 to it so maliit-keyboard can connect.
    raw="''${WAYLAND_DISPLAY:-wayland-0}"
    display="''${raw%%=*}"
    case "$display" in
      /*) socket="$display" ;;
      "") socket="$rt/wayland-0" ;;
      *) socket="$rt/$display" ;;
    esac
    i=0
    while [ "$i" -lt ${toString (cfg.keyboardSocketWaitSeconds * 10)} ]; do
      if [ -S "$socket" ] || [ -S "$rt/wayland-0" ]; then
        exit 0
      fi
      if [ "$raw" != "$display" ] && [ -S "$rt/$raw" ] && [ "$socket" = "$rt/$display" ]; then
        ${pkgs.coreutils}/bin/ln -sfn "$raw" "$socket"
        exit 0
      fi
      ${pkgs.coreutils}/bin/sleep 0.1
      i=$((i + 1))
    done
    echo "fp6: no Wayland compositor socket ($socket); not starting maliit-keyboard" >&2
    exit 1
  '';

  mkcalPlugins = import ./alarms-mkcal-plugins.nix { inherit pkgs; };

  pinCheck = pkgs.runCommand "fp6-lomiri-launcher-pins-check" { } ''
    for id in ${lib.escapeShellArgs cfg.launcherPins}; do
      test -e ${config.system.path}/share/applications/$id.desktop || {
        echo "launcher pin $id.desktop is not in the system profile" >&2
        exit 1
      }
    done
    touch $out
  '';
in {
  options.hardware.fairphone6.lomiriMisc = {
    enable = lib.mkEnableOption "FP6 Lomiri misc fixes (launcher default pins, greeter keyboard start)";
    launcherPins = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "lomiri-dialer-app"
        "lomiri-messaging-app"
        "lomiri-addressbook-app"
        "lomiri-camera-app"
        "lomiri-gallery-app"
        "morph-browser"
        "firefox"
        "lomiri-system-settings"
        "lomiri-terminal-app"
      ];
      description = ''
        Desktop file ids (without .desktop) pinned by default in the Lomiri
        launcher. Each must exist in the system profile (checked at build).
        Set to [ ] to keep Lomiri's stock default.
      '';
    };
    greeterKeyboard = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Only start lomiri-keyboard once a Wayland compositor socket exists.";
    };
    hideIndicatorsClient = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Hide Lomiri's indicators-client test app from the app drawer.";
    };
    alarmsMkcal = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "UITK alarms (Clock app) in mkcal, where indicator-datetime rings them: ALARM_BACKEND=mkcal plus the qtorganizer-mkcal plugin.";
    };
    keyboardSocketWaitSeconds = lib.mkOption {
      type = lib.types.ints.between 1 30;
      default = 5;
      description = "How long lomiri-keyboard's ExecCondition waits for the Wayland socket.";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    (lib.mkIf (cfg.launcherPins != [ ]) {
      programs.dconf.enable = true;
      programs.dconf.profiles.user.databases = [{
        settings."com/lomiri/shell/launcher".items = launcherItems;
      }];
      system.checks = [ pinCheck ];
      system.build.fp6-lomiri-launcher-items = launcherItems;
    })
    (lib.mkIf cfg.greeterKeyboard {
      systemd.user.services.lomiri-keyboard.serviceConfig.ExecCondition = "${waitWaylandSocket}";
      system.build.fp6-lomiri-keyboard-wait-wayland = waitWaylandSocket;
    })
    (lib.mkIf cfg.hideIndicatorsClient {
      nixpkgs.overlays = [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri = lprev.lomiri.overrideAttrs (old: {
            postInstall = (old.postInstall or "") + ''
              f=$out/share/applications/indicators-client.desktop
              grep -qx 'NoDisplay=false' "$f"
              sed -i 's/^NoDisplay=false$/NoDisplay=true/' "$f"
            '';
          });
        });
      }) ];
    })
    (lib.mkIf cfg.alarmsMkcal {
      environment.sessionVariables.ALARM_BACKEND = "mkcal";
      # Profile-relative, so it joins the QT_PLUGIN_PATH string set in
      # configuration.nix (maliit) instead of conflicting with it.
      environment.systemPackages = [ mkcalPlugins ];
      environment.profileRelativeSessionVariables.QT_PLUGIN_PATH = [ "/${mkcalPlugins.pluginRoot}" ];
      system.build.fp6-alarms-mkcal-plugins = mkcalPlugins;
    })
    {
      assertions = [{
        assertion = config.services.desktopManager.lomiri.enable
          && config.systemd.user.services.lomiri-keyboard.enable;
        message = "lomiri-misc-fp6.nix only applies to the Lomiri session with lomiri-keyboard.service.";
      }];
    }
  ]);
}
