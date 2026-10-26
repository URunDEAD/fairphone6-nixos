# Lomiri fingerprint integration: biometryd (com.ubports.biometryd.Service) with
# a device plugin that serves it from fprintd's API (net.reactivated.Fprint,
# provided by fingerprintd -- see fingerprint-persist-fp6.nix).
#
# With this, Lomiri's own UI works unchanged:
#   * System Settings > Security & Privacy > Lock security > Fingerprint ID
#     (lomiri-settings-components' Fingerprint pages): add (guided progress,
#     touch ~20 times), list, rename, delete one, remove all;
#   * the "Fingerprint" unlock toggle in the same page (AccountsService
#     EnableFingerprintIdentification; needs a passcode/password lock);
#   * the greeter: Biometryd.defaultDevice.identifier.identifyUser() on the lock
#     screen, unlocking on a match for the selected user.
#
# Why the env var: biometryd's request verifier reads the caller's AppArmor
# label via org.freedesktop.DBus.GetConnectionAppArmorSecurityContext.
# dbus-broker on this system does no AppArmor mediation and answers with an
# error, upstream then leaves the label unset (and dereferences it). The
# upstream switch below makes that case "unconfined" -- which is exactly what
# every caller is here -- and biometryd still only lets an unconfined caller
# act on its OWN uid's templates.
#
# The QML Biometryd singleton connects once, when a process first imports it
# (falls back to "unavailable" if the service is not there), so biometryd must
# be up before the Lomiri session starts: it is ordered before display-manager.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6BiometrydFprintd;
  biometryd = pkgs.lomiri.biometryd;
  plugin = pkgs.callPackage ./biometryd-fprintd/package.nix { inherit biometryd; };
  # biometryd 0.4.0 scans two compiled-in plugin directories at start and
  # registers each plugin under its descriptor name: its own
  # lib/biometryd/plugins and /custom/vendor/biometryd/plugins (the "vendor"
  # slot meant for exactly this). The generic {"id":"Plugin","config":{"path":..}}
  # route cannot work in 0.4.0: cmds/run.cpp nests the device config under
  # "config" while the Plugin descriptor reads "path" at the top level.
  vendorPluginDir = "/custom/vendor/biometryd/plugins";
  configFile = pkgs.writeText "biometryd-fp6-fprintd.json" (builtins.toJSON {
    defaultDevice = { id = "fp6-fprintd"; config = { }; };
  });
  appFor = systemBusAddress: pkgs.libsForQt5.callPackage ./fingerprint-id-app/package.nix {
    inherit biometryd systemBusAddress;
    inherit (pkgs.lomiri) lomiri-ui-toolkit lomiri-settings-components gsettings-qt lomiri-schemas;
  };
in {
  options.services.fp6BiometrydFprintd = {
    enable = lib.mkEnableOption "biometryd with the fprintd-backed device plugin (Lomiri Fingerprint ID + greeter fingerprint unlock)";
    greeterFeedback = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Patch the Lomiri greeter's cover page with fingerprint feedback: an icon
        while biometryd identification runs, a pulse on an undecided touch,
        shake + red on a rejected finger, green on a match; and let the
        "Try again" hint replace the charging line instead of being hidden by it.
      '';
    };
    app = lib.mkOption {
      type = lib.types.bool;
      default = false;   # Settings has the same pages natively
      description = "Also install the standalone \"Fingerprint ID\" app (the same Lomiri pages as Settings).";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.services.fp6FingerprintPersist.enable or false;
      message = "fp6BiometrydFprintd needs the fingerprint daemon (services.fp6FingerprintPersist).";
    }];

    nixpkgs.overlays = lib.mkIf cfg.greeterFeedback (lib.mkAfter [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-greeter-fingerprint-feedback.patch ];
          postPatch = (old.postPatch or "") + ''
            substituteInPlace qml/Greeter/CoverPage.qml \
              --replace-fail "@FP6_FINGERPRINT_ICON@" \
                "file://${final.adwaita-icon-theme}/share/icons/Adwaita/symbolic/devices/auth-fingerprint-symbolic.svg"
          '';
        });
      });
    }) ]);

    # com.ubports.biometryd.Service.conf (root owns the name, everyone may call it).
    services.dbus.packages = [ biometryd ];

    system.build.fp6-biometryd-fprintd = plugin;
    system.build.fp6-biometryd-config = configFile;
    system.build.fp6-fingerprint-id = appFor null;
    # Only for live tests with biometryd on the private bus /run/fp6-biomtest/bus.
    system.build.fp6-fingerprint-id-privatebus = appFor "unix:path=/run/fp6-biomtest/bus";
    environment.systemPackages = [ plugin ] ++ lib.optional cfg.app (appFor null);

    systemd.tmpfiles.rules = [
      "d /custom 0755 root root - -"
      "d /custom/vendor 0755 root root - -"
      "d /custom/vendor/biometryd 0755 root root - -"
      "L+ ${vendorPluginDir} - - - - ${plugin}/lib/biometryd/plugins"
    ];

    systemd.services.biometryd = {
      description = "biometryd: biometric device mediation (fprintd backend)";
      wantedBy = [ "multi-user.target" ];
      before = [ "display-manager.service" ];
      after = [ "dbus.service" "systemd-tmpfiles-setup.service" ];
      unitConfig.AssertPathExists = [ "${vendorPluginDir}/libfp6-biometryd-fprintd.so" ];
      requires = [ "dbus.service" ];
      # fingerprintd is NOT required: the plugin connects per operation and
      # reports "fprintd unavailable" while it is down.
      environment.BIOMETRYD_DBUS_SKELETON_IS_RUNNING_UNDER_TESTING = "1";
      serviceConfig = {
        Type = "dbus";
        BusName = "com.ubports.biometryd.Service";
        ExecStart = "${biometryd}/bin/biometryd run --config=${configFile}";
        Restart = "on-failure";
        RestartSec = 2;
        RestartPreventExitStatus = 78;   # EX_CONFIG: device could not be created
        # Only needs the system bus.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        PrivateNetwork = false;          # D-Bus unix socket
        RestrictAddressFamilies = [ "AF_UNIX" ];
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        LockPersonality = true;
      };
    };
  };
}
