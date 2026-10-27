# Wi-Fi QR codes (feature push 2026-10-24): share a saved network as a QR
# code, scan a code to join. Notes, previews and the phone test list:
# results/feature-push-20261024/wifi-qr/README.md
#
#  * Share: Settings > Wi-Fi > connected network (or > Previous networks >
#    network) > "Share (QR code)". First "Confirm it's you": fingerprint
#    (biometryd identify, as the greeter) when fingerprint unlock is on, or
#    the passcode/passphrase (PAM service fp6-wifi-qr: pam_unix, the same
#    account password the greeter checks; 5 free tries, then 30 s doubling).
#    No screen lock (swipe) or an open network: no question, like Android.
#    Only then the password comes from NetworkManager (GetSecrets) and the
#    page shows the standard code WIFI:T:WPA|SAE|WEP|nopass;S:..;P:..;H:..;;
#    (ZXing escaping), the name and the password (hidden until tapped).
#    The stock "Show password" switch (which showed every saved password
#    without asking) asks the same way now.
#  * Scan: Settings > Wi-Fi > "Scan QR code to connect" and the network
#    indicator menu ("Scan QR code to connect…", patched
#    lomiri-indicator-network service binary only) open fp6-wifi-qr-scan:
#    Lomiri Camera's camera stack (as the Magnifier), frames -> Rust QR
#    decoder; "Join <ssid>?" -> NetworkManager AddAndActivateConnection2
#    (in memory, saved once connected; nothing kept on a wrong password).
#    New networks get privacy-lock's stable private MAC; a saved network with
#    the same SSID keeps its address. Enterprise codes: "unsupported".
#    Desktop entry name "Wi-Fi QR scanner" = the camera in-use indicator.
#
# Overlay lib.mkOrder 2950: after battery/notifications (2900) and
# privacy-lock (2800), before storage-backup (3000). The stock Wi-Fi QML
# (PageComponent, NetworkDetailsBrief, NetworkDetails) is patched in the
# lomiri-system-settings wrapper (lss-wifi-qr.patch fails the build if
# upstream changed them); no C++ rebuild of System Settings or Lomiri.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.wifiQr;
  cameraApp = pkgs.lomiri.lomiri-camera-app;
  haveCamera = (config.hardware.fp6LomiriCamera.enable or false) && cameraApp ? qtmultimediaFp6;
  privacyMac = (config.hardware.fairphone6.privacyLock.enable or false)
    && (config.hardware.fairphone6.privacyLock.wifiMac or false);
  pkg = pkgs.callPackage ./package.nix ({
    assignedMac = if privacyMac then "stable" else "";
  } // lib.optionalAttrs haveCamera {
    camera = cameraApp;
    cameraLibcamera = config.hardware.fp6Camera.package;
  });
  scannerOn = cfg.scanner && haveCamera;

  # On top of network-checks' hotspot-channel build when that module is in
  # (it swaps the same service binary), else on the stock package.
  indicatorBase = config.system.build.fp6-indicator-network-hotspot or pkgs.lomiri.lomiri-indicator-network;
  indicatorNetwork = indicatorBase.overrideAttrs (old: {
    pname = "${old.pname}-wifiqr";
    patches = (old.patches or [ ]) ++ [ ./indicator-network-scan-qr.patch ];
    # The menu-structure tests pin the stock Wi-Fi section.
    doCheck = false;
  });
in
{
  options.hardware.fairphone6.wifiQr = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Wi-Fi QR codes: "Share (QR code)" for saved networks (after the
        passcode/fingerprint) and "Scan QR code to connect" (camera scanner)
        in System Settings > Wi-Fi and the network indicator.'';
    };
    scanner = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install the fp6-wifi-qr-scan camera scanner (needs hardware.fp6LomiriCamera).";
    };
    indicatorEntry = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        "Scan QR code to connect…" in the network indicator's Wi-Fi section
        (a patched lomiri-indicator-network service binary, started through a
        drop-in, stacked on network-checks' hotspot patch when present;
        nothing that links lomiri-connectivity-qt is rebuilt).'';
    };
    apparmor = lib.mkOption {
      type = lib.types.enum [ "disable" "complain" "enforce" ];
      default = if config.hardware.fairphone6.apparmor.enable or false then "complain" else "disable";
      defaultText = lib.literalExpression ''"complain" when hardware.fairphone6.apparmor.enable'';
      description = "Mode of the fp6-wifi-qr-scan AppArmor profile (complain until exercised live).";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      nixpkgs.overlays = lib.mkOrder 2950 [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
            plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
          })).overrideAttrs (old: {
            nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.buildPackages.patch ];
            # Biometryd QML for the fingerprint confirmation.
            buildInputs = (old.buildInputs or [ ]) ++ [ lprev.biometryd ];
            postInstall = (old.postInstall or "") + ''
              w=$out/share/lomiri-system-settings/qml-plugins/wifi
              for f in PageComponent.qml NetworkDetailsBrief.qml NetworkDetails.qml; do
                src=$(readlink -f "$w/$f")
                cp --remove-destination "$src" "$w/$f.fp6"
                mv "$w/$f.fp6" "$w/$f"
                chmod u+w "$w/$f"
              done
              patch -d "$w" -p1 --forward --batch < ${./lss-wifi-qr.patch}
              grep -q Fp6WifiScanItem "$w/PageComponent.qml"
              grep -q Fp6WifiDetailsExtras "$w/NetworkDetailsBrief.qml" "$w/NetworkDetails.qml"
              test -e "$w/Fp6WifiSharePage.qml"
              test -e $out/lib/lomiri-system-settings/private/Fp6/WifiQr/qmldir
            '';
          });
        });
      }) ];

      # Passcode/passphrase check before a saved password is shown or shared.
      security.pam.services.fp6-wifi-qr.text = ''
        auth     required ${pkgs.pam}/lib/security/pam_unix.so
        account  required ${pkgs.pam}/lib/security/pam_unix.so
      '';

      system.build.fp6-wifi-qr = {
        inherit (pkg) core lssPlugin scanner;
        inherit indicatorNetwork;
        inherit (pkgs.lomiri) lomiri-system-settings;
      };
    }

    (lib.mkIf scannerOn {
      environment.systemPackages = [ pkg.scanner ];
    })

    (lib.mkIf (scannerOn && cfg.indicatorEntry && config.networking.networkmanager.enable) {
      # Same unit, patched binary. Forced: network-checks sets this ExecStart
      # to its hotspot build, which indicatorNetwork already contains.
      systemd.user.services.lomiri-indicator-network = {
        overrideStrategy = "asDropin";
        serviceConfig.ExecStart = lib.mkForce [ "" "${indicatorNetwork}/libexec/lomiri-indicator-network/lomiri-indicator-network-service" ];
      };
    })

    (lib.mkIf (scannerOn && cfg.apparmor != "disable") {
      security.apparmor.policies.fp6-wifi-qr-scan = {
        state = cfg.apparmor;
        path = ./fp6-wifi-qr-scan.apparmor;
      };
      system.checks = [
        (pkgs.runCommand "fp6-wifi-qr-scan-apparmor-compile" { } ''
          ${pkgs.apparmor-parser}/bin/apparmor_parser --config-file /dev/null -Q -K \
            -I ${config.environment.etc."apparmor.d".source} \
            ${lib.concatMapStringsSep " " (p: "-I ${p}/etc/apparmor.d") config.security.apparmor.packages} \
            ${./fp6-wifi-qr-scan.apparmor} 2>&1 | { grep -v 'Cache read/write disabled' || true; }
          test "''${PIPESTATUS[0]}" = 0
          touch $out
        '')
      ];
    })
  ]);
}
