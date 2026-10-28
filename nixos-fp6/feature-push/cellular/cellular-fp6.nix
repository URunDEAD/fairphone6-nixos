# FP6 (SM7635 "milos") cellular + GNSS for the Lomiri target.
#
# STACK DECISION: upstream oFono 2.19 (plugins/qrtrqmi.c over QRTR node 0) is
# the ONE modem owner. Lomiri's whole telephony stack speaks the oFono API
# (telepathy-ofono -> telephony-service/history-service, lomiri-indicator-
# network via libqofono + NetworkManager's oFono backend, nuntium for MMS via
# org.ofono.PushNotification + MMS contexts). ModemManager (which the old
# postmarketOS root used with its qcom-soc plugin) has no maintained
# MM->oFono bridge (oFono2MM goes the other way), so MM is forced OFF here.
# Cost: imsd (userspace VoLTE) is ModemManager-only -> VoLTE is NOT provided
# by this module (see results/feature-push-20261024/cellular/README.md).
#
# GNSS: raw QMI LOC over QRTR (fp6-loc-nmea.py), streamed to GeoClue's
# [network-nmea] nmea-socket, started on demand only while GeoClue listens.
#
# NV / EFS SAFETY (why default-on is acceptable):
#  * rmtfs from configuration.nix runs "-r -P -s": -P serves the by-partlabel
#    modemst1/modemst2/fsg/fsc/study partitions, -r opens them O_RDONLY and
#    keeps every modem EFS write in a RAM shadow buffer (storage_pwrite ->
#    memcpy into shadow_buf; storage_sync is a no-op). Nothing the modem, oFono
#    or this module does can reach modemst1/2 or fsg on UFS. An assertion
#    below refuses to build if rmtfs ever loses "-r" while this is enabled.
#  * Consequence: modem-side persistent settings (oFono's LTE attach-APN
#    WDS ModifyProfile, PDC/MCFG auto-selection, network-selection caches)
#    are re-learned each boot; that is the intended trade.
#  * Only exception (user-approved 2026-10-26): LOC engine lock "mt" per boot
#    (fp6-loc-engine-lock, RAM EFS only). It never sets NMEA types / operation mode, never
#    edits NV items, IMEI, carrier/SIM lock, PDC configs or eSIM profiles.
#  * -s means rmtfs itself starts/stops the mpss remoteproc (unchanged).
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6Cellular;
  fp6 = import ./packages.nix { inherit pkgs; };
  rmtfsExec = toString (config.systemd.services.rmtfs.serviceConfig.ExecStart or "");
  ofonoDbusPolicy = pkgs.writeTextDir "share/dbus-1/system.d/fp6-ofono-radio.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <!-- at_console is a no-op on current dbus: grant the phone user's
           "radio" group explicit access, as Ubuntu Touch does. -->
      <policy group="radio">
        <allow send_destination="org.ofono"/>
      </policy>
    </busconfig>
  '';
in {
  imports = [
    ../../telephony-fp6.nix            # telepathy-ofono + libofono-qt
    ../../mms-fp6.nix                  # nuntium user service
    ../../telephony-frontends-fp6.nix  # Lomiri dialer / messaging / contacts
    ./messaging-fp6.nix                # SMS notifications + telepathy-ofono SQLite
    ./mms-fp6.nix                      # nuntium fixes (modem-removal panic, MMSC response text)
    ./agps/agps-fp6.nix                # assisted GPS (XTRA / time / coarse position)
    ./rmtfs-shadow/rmtfs-shadow-fp6.nix # persistent read-only EFS shadow (hardware.fairphone6.modem.persistShadow, default off)
  ];

  options.services.fp6Cellular = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = config.services.desktopManager.lomiri.enable;
      defaultText = lib.literalExpression "config.services.desktopManager.lomiri.enable";
      description = "oFono-over-QRTR cellular stack for Lomiri (calls, SMS, data, MMS) on FP6.";
    };
    radioUser = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "Phone user added to the radio group (system-bus access to org.ofono).";
    };
    uimSelection = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Run the pmOS-derived UIM provisioning-session selection before oFono.";
    };
    premuxCount = lib.mkOption {
      type = lib.types.ints.between 3 8;
      default = 4;
      description = "rmnet_dataN premux links on rmnet_ipa0 (oFono needs >= 3).";
    };
    ofonoDebug = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Run ofonod with -d (verbose; may log subscriber data - bring-up only).";
    };
    mms = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable nuntium (MMS) on top of oFono.";
    };
    gnss = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "GeoClue NMEA source fed by QMI LOC (on-demand GNSS session).";
      };
      allowedApplications = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "GeoClue desktop IDs allowed for the phone user (no blanket grant).";
      };
      userUID = lib.mkOption {
        type = lib.types.str;
        default = "10000";
      };
      fixCache = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "fp6-loc-nmea --fix-cache: file for the last fix, rounded to ~5 km (set by services.fp6Agps.lastFix).";
      };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      assertions = [
        { assertion = lib.hasInfix " -r" rmtfsExec;
          message = "fp6Cellular: rmtfs must run read-only (-r) so modem EFS writes never reach modemst1/modemst2/fsg. Got: ${rmtfsExec}"; }
        { assertion = !config.networking.modemmanager.enable;
          message = "fp6Cellular: oFono owns the QRTR modem; ModemManager must stay disabled (single owner)."; }
        { assertion = !(config.services.fp6Imsd.enable or false) && !((config.services.fp6Imsd.ofonoBridge.enable or false));
          message = "fp6Cellular: imsd/imsd-ofonod require ModemManager or claim org.ofono; not combinable with oFono."; }
        { assertion = config.services.desktopManager.lomiri.enable;
          message = "fp6Cellular targets the Lomiri session."; }
      ];

      networking.modemmanager.enable = lib.mkForce false;

      # ---- oFono --------------------------------------------------------
      services.ofono.enable = true;
      services.dbus.packages = [ ofonoDbusPolicy ];
      users.groups.radio = { };
      users.users.${cfg.radioUser}.extraGroups = [ "radio" ];

      # ---- modem firmware service + MCFG activation ---------------------
      # The overlay's 2020 tqftpserv ignores TFTP rsize/seek; the FP6 modem
      # then never loads mcfg_hw and stays DMS "offline" with dead UIM slots
      # (results/feature-push-20261024/cellular/esim-README.md).
      systemd.services.tqftpserv = {
        before = [ "rmtfs.service" ];
        serviceConfig = {
          ExecStart = lib.mkForce "${fp6.tqftpserv}/bin/tqftpserv";
        };
      };
      systemd.services.rmtfs = {
        after = [ "tqftpserv.service" ];
        wants = [ "tqftpserv.service" ];
        # The modem's /readwrite/ scratch (mcfg.tmp = MCFG selection state)
        # must live exactly as long as rmtfs's RAM EFS shadow: a stale
        # mcfg.tmp next to a fresh EFS makes the modem skip MCFG selection
        # and stay offline. tqftpserv recreates the directory on demand.
        serviceConfig.ExecStartPre = [ "-${pkgs.coreutils}/bin/rm -rf /run/tqftpserv" ];
      };
      systemd.services.fp6-modem-mcfg-activate = {
        description = "FP6 modem: activate auto-selected platform MCFG (one remoteproc restart)";
        wantedBy = [ "multi-user.target" "rmtfs.service" ];
        # PartOf: an rmtfs restart drops the RAM EFS shadow, so redo this.
        partOf = [ "rmtfs.service" ];
        after = [ "rmtfs.service" "tqftpserv.service" ];
        requires = [ "rmtfs.service" ];
        before = [ "msm-modem-uim-selection.service" "ofono.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = lib.getExe fp6.fp6-modem-mcfg-activate;
          TimeoutStartSec = 900;
        };
      };

      systemd.services.msm-modem-uim-selection = lib.mkIf cfg.uimSelection {
        description = "MSM modem UIM provisioning-session selection (runtime only)";
        wantedBy = [ "multi-user.target" ];
        after = [ "rmtfs.service" "fp6-modem-mcfg-activate.service" ];
        requires = [ "rmtfs.service" ];
        before = [ "ofono.service" ];
        environment.SIM_WAIT_TIME = "30";
        environment.USIM_WAIT_TIME = "15";
        # Exit 75 = card present but no USIM app yet (e.g. eUICC right after a
        # profile enable): retry every 30 s for up to ~10 min. After enabling a
        # profile later at runtime: systemctl restart msm-modem-uim-selection.
        startLimitIntervalSec = 900;
        startLimitBurst = 20;
        serviceConfig = {
          Type = "notify";
          NotifyAccess = "all";
          ExecStart = lib.getExe fp6.msm-modem-uim-selection;
          TimeoutStartSec = 180;
          Restart = "on-failure";
          RestartSec = 30;
        };
      };

      # Data path (results/feature-push-20261024/cellular/data-sms-README.md):
      #  ExecStartPre  - DPM OpenPort (IPA pipes 2/18) + WDA QMAPv5 on
      #                  embedded/1, then rmnet_data0..N-1 with MAPv5 csum
      #                  flags so udevng's qrtrsoc premux detection sees them;
      #  ExecStartPost - oFono's rmnet_init deletes every pre-existing rmnet
      #                  link right after start; recreate them (same names /
      #                  mux ids, which the modem object keeps).
      # A modem SSR re-registers rmnet_ipa0 (and drops the DPM/WDA state and
      # the rmnet children); the udev rule below restarts oFono then, which
      # re-runs the whole chain. At boot oFono is not active yet -> no-op.
      systemd.services.ofono = {
        wantedBy = [ "multi-user.target" ];
        after = [ "rmtfs.service" "fp6-modem-mcfg-activate.service" "msm-modem-uim-selection.service" ];
        serviceConfig = {
          StandardError = "journal";
          StateDirectory = "ofono";
          StateDirectoryMode = "0700";
          ExecStartPre = [
            "-${lib.getExe fp6.fp6-modem-data-port} port"
            "-${lib.getExe fp6.fp6-modem-data-port} links ${toString cfg.premuxCount}"
          ];
          ExecStart = [ "" "${fp6.ofono}/bin/ofonod -n${lib.optionalString cfg.ofonoDebug " -d"}" ];
          ExecStartPost = [ "-${lib.getExe fp6.fp6-modem-data-port} relink ${toString cfg.premuxCount}" ];
          TimeoutStartSec = 180;
          Restart = "on-failure";
          RestartSec = 5;
        };
      };
      # Nothing else powers the oFono modem at boot (Lomiri's network indicator
      # only toggles Online for flight mode), so the SIM showed "not inserted"
      # until Powered/Online were set by hand (2026-10-26). Re-runs with ofono.
      systemd.services.fp6-ofono-online = {
        description = "FP6: power the oFono modem and bring it online";
        wantedBy = [ "ofono.service" ];
        after = [ "ofono.service" ];
        bindsTo = [ "ofono.service" ];
        path = [ pkgs.systemd pkgs.gnugrep pkgs.coreutils ];
        serviceConfig = { Type = "oneshot"; RemainAfterExit = true; };
        script = ''
          for i in $(seq 1 60); do
            m=$(busctl --system call org.ofono / org.ofono.Manager GetModems 2>/dev/null | grep -o '"/qrtrsoc_[0-9]*"' | head -1 | tr -d '"')
            [ -n "$m" ] && break
            sleep 2
          done
          [ -n "$m" ] || { echo "no oFono modem appeared"; exit 0; }
          busctl --system call org.ofono "$m" org.ofono.Modem SetProperty sv Powered b true
          sleep 5
          busctl --system call org.ofono "$m" org.ofono.Modem SetProperty sv Online b true
          echo "$m powered and online"
        '';
      };
      services.udev.extraRules = ''
        ACTION=="add", SUBSYSTEM=="net", KERNEL=="rmnet_ipa0", RUN+="${pkgs.systemd}/bin/systemctl --no-block try-restart ofono.service"
      '';
      # ---- mobile data policy: one owner -------------------------------
      # lomiri-indicator-network-service is the data policy manager: it keeps
      # its own per-user MobileDataEnabled and forces oFono's
      # ConnectionManager.Powered back to it on every change. The pre-login
      # greeter (lightdm user, own ~/.config/connectivity-service, default
      # "false") ran a second instance next to the user's; after a
      # display-manager restart both fought over Powered thousands of times
      # (NetworkManager's gsm device flapped and gave up). oFono already
      # stores Powered persistently per SIM, so the greeter does not need a
      # policy instance at all: the user's choice stays system-wide.
      # Cost: no network indicator / SIM-PIN prompt on the pre-login greeter
      # (the user session prompts after login).
      systemd.user.services.lomiri-indicator-network = {
        overrideStrategy = "asDropin";
        unitConfig.ConditionUser = "!lightdm";
      };

      # telepathy-ofono's ofono.manager must be visible to mission-control
      # (XDG_DATA_DIRS -> /run/current-system/sw/share/telepathy/managers).
      environment.pathsToLink = [ "/share/telepathy" ];

      # ---- data: NM with oFono backend (contexts -> gsm connections) ------
      networking.networkmanager.enable = true;
      networking.networkmanager.package = fp6.networkmanager-ofono;

      # ---- Lomiri telephony middleware + apps ------------------------------
      services.fp6TelephonyMiddleware.enable = true;
      programs.fp6TelephonyFrontends.enable = lib.mkDefault true;
      services.fp6Mms.enable = cfg.mms;

      # System Settings > Cellular: network mode ("2G/3G/4G/5G" via oFono
      # RadioSettings), APNs, data roaming. Added as a wrapper plugin, so
      # only the lomiri-system-settings wrapper is rebuilt.
      nixpkgs.overlays = [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (lfinal: lprev: {
          lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
            plugins = (old.plugins or [ ]) ++ [
              (lfinal.callPackage ./lomiri-system-settings-cellular.nix {
                inherit (lprev) lomiri-system-settings-unwrapped;
              })
            ];
          });
        });
      }) ];

      environment.systemPackages = [ fp6.ofono pkgs.libqmi fp6.fp6-loc-nmea ];
      system.build.fp6-cellular = {
        inherit (fp6) tqftpserv fp6-modem-mcfg-activate fp6-modem-data-port ofono fp6-loc-nmea fp6-location-indicator networkmanager-ofono msm-modem-uim-selection fp6-ofono-account-wrapped;
      };

      systemd.user.services.fp6-ofono-account = {
        description = "Create Telepathy accounts for real oFono modems";
        wantedBy = [ "graphical-session.target" ];
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        serviceConfig = {
          ExecStart = lib.getExe' fp6.fp6-ofono-account-wrapped "fp6-ofono-account";
          Restart = "on-failure";
          RestartSec = 15;
        };
      };
    }

    # ---- GNSS -------------------------------------------------------------
    (lib.mkIf cfg.gnss.enable {
      services.geoclue2 = {
        enable = true;
        enableNmea = true;          # [network-nmea] enable=true
        enableModemGPS = false;     # that source needs ModemManager
        enable3G = false;
        enableCDMA = false;
        enableWifi = lib.mkDefault false;   # no Mozilla/BeaconDB lookups by default
        # With the WiFi source off and no static source, geoclue still made
        # a GeoIP-only source with the (empty) locate URL: it could never
        # look anything up and logged "Could not parse locate URL ''" on
        # every network change. The patch skips that source when no URL is
        # set (agps networkLocation sets one, and the WiFi source with it).
        # mkDefault: gps-fp6's modemOnly package wins if both are enabled.
        package = lib.mkDefault (pkgs.geoclue2.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./geoclue-no-geoip-without-url.patch ];
        }));
        submitData = false;
        enableDemoAgent = lib.mkDefault false;
        # fp6-location-indicator is the user's GeoClue agent (the toggle).
        whitelistedAgents = [ "fp6-location-indicator" ];
        appConfig = lib.genAttrs cfg.gnss.allowedApplications (_: {
          isAllowed = true;
          isSystem = false;
          users = [ cfg.gnss.userUID ];
        });
      };
      environment.etc."geoclue/conf.d/90-fp6-gnss.conf".text = ''
        [network-nmea]
        enable=true
        nmea-socket=/run/fp6-gnss/nmea.sock
      '';
      # LOC engine lock: the modem boots with "mi" (mobile-initiated
      # sessions locked), so every LOC session from the AP is refused.
      # USER-APPROVED (2026-10-26) exception to the module's no-modem-config
      # rule, identical to postmarketOS's modem-gnss.service: set "mt" once
      # per boot. With rmtfs -r it only reaches the RAM EFS shadow. No other
      # LOC/NV setting is touched.
      systemd.services.fp6-loc-engine-lock = {
        description = "FP6 modem GNSS: allow AP (mobile-initiated) location sessions";
        wantedBy = [ "multi-user.target" "rmtfs.service" ];
        partOf = [ "rmtfs.service" ];
        after = [ "rmtfs.service" "fp6-modem-mcfg-activate.service" ];
        before = [ "fp6-gnss-nmea.service" "geoclue.service" ];
        startLimitIntervalSec = 600;
        startLimitBurst = 10;
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${pkgs.coreutils}/bin/timeout 30 ${pkgs.libqmi}/bin/qmicli -d qrtr://0 --loc-set-engine-lock=mt";
          Restart = "on-failure";
          RestartSec = 20;
        };
      };
      # Location detection is OFF by default: this unit is NOT wanted at boot.
      # The Lomiri location toggle (status-bar indicator and System Settings >
      # Security & Privacy > Location, both via fp6-location-indicator) starts
      # and stops it. While it runs, each GeoClue connection gets one LOC
      # session (fp6-loc-nmea); stopping the unit ends the session (LOC Stop).
      systemd.services.fp6-gnss-nmea = {
        description = "FP6 modem GNSS NMEA socket for GeoClue (QMI LOC over QRTR)";
        after = [ "rmtfs.service" "fp6-loc-engine-lock.service" ];
        wants = [ "fp6-loc-engine-lock.service" ];
        before = [ "geoclue.service" ];
        serviceConfig = {
          User = "geoclue";
          Group = "geoclue";
          RuntimeDirectory = "fp6-gnss";
          RuntimeDirectoryMode = "0750";
          ExecStart = "${lib.getExe pkgs.socat} UNIX-LISTEN:/run/fp6-gnss/nmea.sock,fork,unlink-early,mode=0660 \"EXEC:${lib.getExe fp6.fp6-loc-nmea}${lib.optionalString (cfg.gnss.fixCache != null) " --fix-cache ${cfg.gnss.fixCache}"}\"";
          Restart = "on-failure";
          RestartSec = 5;
          SuccessExitStatus = [ 143 ];   # socat on SIGTERM (toggle off)
          RestrictAddressFamilies = [ "AF_UNIX" "AF_QIPCRTR" ];
          NoNewPrivileges = true;
          ProtectSystem = "strict";
          ProtectHome = true;
          PrivateTmp = true;
        };
      };
      # The phone user (radio group) may start/stop exactly this unit.
      security.polkit.extraConfig = ''
        polkit.addRule(function(action, subject) {
          if (action.id == "org.freedesktop.systemd1.manage-units" &&
              action.lookup("unit") == "fp6-gnss-nmea.service" &&
              (action.lookup("verb") == "start" || action.lookup("verb") == "stop" ||
               action.lookup("verb") == "restart") &&
              subject.isInGroup("radio")) {
            return polkit.Result.YES;
          }
        });
      '';
      environment.systemPackages = [ fp6.fp6-location-indicator ];
      environment.pathsToLink = [ "/share/ayatana" ];
      systemd.user.services.fp6-location-indicator = {
        description = "Location indicator (Lomiri toggle for modem GPS)";
        partOf = [ "graphical-session.target" "ayatana-indicators.target" "lomiri-indicators.target" ];
        wantedBy = [ "ayatana-indicators.target" "lomiri-indicators.target" ];
        serviceConfig = {
          ExecStart = lib.getExe fp6.fp6-location-indicator;
          Restart = "on-failure";
          RestartSec = 5;
        };
      };
    })
  ]);
}
