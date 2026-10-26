# Privacy & lock (feature push 2026-10-24). Notes and live results:
# results/feature-push-20261024/privacy-lock/README.md.
#
#  * Auto-lock: Settings > Security & Privacy > Locking and unlocking.
#    "Lock when idle" (com.lomiri.touch.system activity-timeout) now works:
#    fp6-lomiri-powerd dims and turns the display off after it (reason
#    inactivity) and Lomiri locks. New: "Lock after the display turns off"
#    (grace period, default immediately), "Power button locks instantly"
#    (default on), both in Lomiri's Shell.qml; touches are swallowed while
#    the display is off. The passcode/fingerprint is needed once locked.
#  * Emergency information on the lock screen (Emergency button on the
#    passcode screen -> owner message, medical info, emergency contacts with
#    Call, Emergency call), default off. Data: the user's dconf; lock-screen
#    copy /var/lib/fp6-privacy/emergency.json (2750 <user>:lightdm, 0640).
#  * Emergency SOS: N quick power presses -> countdown notification with
#    Cancel -> call (oFono), optional SMS with location to the emergency
#    contacts. Default OFF with a warning dialog; Test mode = mock dialer.
#  * In-use indicator (status bar) for camera / microphone / location with
#    the app names: fp6-privacy-indicator (user) + fp6-privacy-system
#    camera-monitor (root, camss node holders incl. Waydroid).
#  * Wi-Fi MAC: random for scans, stable per network for connections;
#    networks saved before are pinned to their current address once
#    (fp6-wifi-mac-migrate, before NetworkManager). Per-network choice in
#    Settings > Security & Privacy > Private Wi-Fi address.
#
#  * Clipboard: not available on Lomiri's Mir 2.15 (no ext-data-control);
#    fp6-privacy-clipboard is ready for a Mir that has it (option clipboard).
#  * Privileged Wayland extensions (qtmir patch): virtual keyboard/pointer,
#    foreign toplevel, screencopy and input-method-v2 are no longer offered
#    to every app (to nobody).
#  * Lock-screen camera: hold the camera icon on the lock screen ->
#    lomiri-camera-app --secure as the greeter's locked app (default off).
#
# Lomiri patch (lomiri-privacy-lock.patch, QML only): Shell.qml (grace
# timer, power-key option, display-off input block), Greeter/GreeterView.qml
# + Greeter/EmergencyInfo.qml. Overlay lib.mkOrder 2800, after
# lomiri-polish (2000) ... app-ready (2600), before notifications (2900).
# Security & Privacy QML (PageComponent.qml, PhoneLocking.qml) is patched in
# the lomiri-system-settings wrapper (no C++ rebuild).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.privacyLock;
  user = config.services.fp6LomiriSensors.user or "user";
  schema = pkgs.callPackage ./schema.nix { };
  package = pkgs.callPackage ./package.nix { fp6-privacy-schema = schema; };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-privacy.nix { fp6-privacy-schema = schema; };
  indicatorFile = pkgs.runCommand "fp6-privacy-indicator-service" { } ''
    mkdir -p $out/share/ayatana/indicators
    cat > $out/share/ayatana/indicators/com.lomiri.indicator.privacy <<EOF
    [Indicator Service]
    Name=fp6-privacy-indicator
    ObjectPath=/com/lomiri/indicator/privacy
    Position=5

    [phone]
    ObjectPath=/com/lomiri/indicator/privacy/phone
    Position=5

    [desktop]
    ObjectPath=/com/lomiri/indicator/privacy/desktop
    EOF
  '';
  # Lock-screen camera: only with a camera app that has the restricted mode
  # (camera/lomiri-camera-app-secure-mode.patch); otherwise no desktop entry,
  # so Lomiri's lock-screen button cannot start anything.
  cameraSecure = lib.any (p: lib.hasSuffix "lomiri-camera-app-secure-mode.patch" (toString p))
    (pkgs.lomiri.lomiri-camera-app.patches or [ ]);
  secureCameraEntry = pkgs.writeTextDir "share/applications/lomiri-camera-app-secure.desktop" ''
    [Desktop Entry]
    Type=Application
    Name=Camera (locked)
    Comment=Camera from the lock screen: photos of this session only
    Exec=lomiri-camera-app --secure
    Terminal=false
    NoDisplay=true
    Icon=lomiri-camera-app
    X-Lomiri-Touch=true
    X-Lomiri-Single-Instance=true
    X-Lomiri-Rotates-Window-Contents=true
    X-Lomiri-Splash-Image=lomiri-app-launch/splash/lomiri-camera-app.svg
    X-Lomiri-Splash-Color=#eeecea
  '';
in {
  options.hardware.fairphone6.privacyLock = {
    enable = lib.mkEnableOption ''
      privacy & lock features: idle/grace auto-lock options, lock-screen
      emergency information, Emergency SOS (off by default), camera /
      microphone / location in-use indicator, Wi-Fi MAC randomisation'';
    lomiriShell = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Lomiri patch (lomiri-privacy-lock.patch): lock grace period, power key
        option, display-off input block, lock-screen emergency information
        and camera button.'';
    };
    restrictWaylandExtensions = lib.mkOption {
      type = lib.types.bool;
      # Built and checked against Lomiri's Mir 2.15 (qtmir alone, rdxajr8q...).
      default = true;
      description = ''
        qtmir no longer offers zwp_virtual_keyboard_manager_v1,
        zwlr_virtual_pointer, zwlr_foreign_toplevel, zwlr_screencopy and
        zwp_input_method_manager_v2 to every app, to nobody instead
        (qtmir-privileged-wayland-extensions.patch; Maliit uses
        zwp_input_method_v1). Lomiri's
        FP6_WAYLAND_PRIVILEGED_ALLOW_ALL=1 restores the old behaviour.'';
    };
    clipboard = lib.mkOption {
      type = lib.types.bool;
      # Lomiri's Mir 2.15 has no ext-data-control (Mir >= 2.2x has): nothing
      # to run against yet. See README "5. Clipboard".
      default = false;
      description = ''
        fp6-privacy-clipboard: clipboard auto-clear and paste notifications
        over Wayland ext-data-control. Needs a Mir with ext_data_control
        offered to that helper only (not Lomiri's Mir 2.15).'';
    };
    wifiMac = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        NetworkManager defaults: random MAC for scans, stable per-network MAC
        for connections. Saved networks keep their address (pinned once).'';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.services.fp6LomiriSensors.enable or false;
      message = "privacy-lock needs fp6-lomiri-powerd (services.fp6LomiriSensors) for idle display-off and the power key.";
    }];

    nixpkgs.overlays = lib.mkOrder 2800 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        # Privileged Wayland extensions (virtual keyboard/pointer, foreign
        # toplevel, screencopy, input-method-v2) offered to nobody.
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ lib.optional cfg.restrictWaylandExtensions ./qtmir-privileged-wayland-extensions.patch;
        });
        lomiri = if !cfg.lomiriShell then lprev.lomiri else lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-privacy-lock.patch ];
          # wrapGAppsHook: the com.fairphone6.privacy schema in Lomiri's XDG_DATA_DIRS.
          buildInputs = (old.buildInputs or [ ]) ++ [ schema ];
          passthru = (old.passthru or { }) // { fp6PrivacyLock = true; };
        });
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
        })).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.buildPackages.patch ];
          # Replace the two security-privacy links by patched copies; the
          # patch fails the build if upstream changed them.
          postInstall = (old.postInstall or "") + ''
            sp=$out/share/lomiri-system-settings/qml-plugins/security-privacy
            for f in PageComponent.qml PhoneLocking.qml; do
              src=$(readlink -f "$sp/$f")
              cp --remove-destination "$src" "$sp/$f.fp6"
              mv "$sp/$f.fp6" "$sp/$f"
              chmod u+w "$sp/$f"
            done
            patch -d "$sp" -p1 --forward --batch < ${./lss-security-privacy.patch}
            grep -q fp6Privacy "$sp/PageComponent.qml" "$sp/PhoneLocking.qml"
          '';
        });
      });
    }) ];

    environment.systemPackages = [ package indicatorFile ] ++ lib.optional cameraSecure secureCameraEntry;
    environment.pathsToLink = [ "/share/ayatana" ];

    # In-use indicator (status bar).
    systemd.user.services.fp6-privacy-indicator = {
      description = "Camera / microphone / location in-use indicator for Lomiri";
      partOf = [ "graphical-session.target" "ayatana-indicators.target" "lomiri-indicators.target" ];
      wantedBy = [ "ayatana-indicators.target" "lomiri-indicators.target" ];
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = "${package}/bin/fp6-privacy-indicator";
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 5;
      };
    };

    # Emergency info mirror, SOS, Wi-Fi MAC policy.
    systemd.user.services.fp6-privacyd = {
      description = "FP6 privacy & lock helper (emergency info, SOS, Wi-Fi MAC policy)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = "${package}/bin/fp6-privacyd run";
        Restart = "on-failure";
        RestartSec = 5;
      };
    };

    # Clipboard auto-clear / paste notifications. Not wrapped: qtmir compares
    # /proc/<pid>/exe with the real path of the system-profile binary.
    systemd.user.services.fp6-privacy-clipboard = lib.mkIf cfg.clipboard {
      description = "FP6 clipboard auto-clear and paste notifications";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" "lomiri.service" ];
      unitConfig.ConditionUser = user;
      # The user manager's WAYLAND_DISPLAY is empty in this session.
      environment = {
        WAYLAND_DISPLAY = "wayland-0";
        GSETTINGS_SCHEMA_DIR = "${schema}/share/gsettings-schemas/${schema.name}/glib-2.0/schemas";
        GIO_EXTRA_MODULES = "${lib.getLib pkgs.dconf}/lib/gio/modules";
      };
      serviceConfig = {
        ExecStart = "/run/current-system/sw/bin/fp6-privacy-clipboard";
        Restart = "on-failure";
        RestartSec = 10;
        Nice = 5;
      };
    };

    # Camera holders (root: /proc/<pid>/fd of every process, Waydroid too).
    systemd.services.fp6-privacy-camera-monitor = {
      description = "FP6 camera in-use monitor (camss capture node holders)";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udev-settle.service" "systemd-tmpfiles-setup.service" ];
      serviceConfig = {
        ExecStart = "${package}/bin/fp6-privacy-system camera-monitor";
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 10;
        CapabilityBoundingSet = [ "CAP_SYS_PTRACE" "CAP_DAC_READ_SEARCH" ];
        AmbientCapabilities = [ ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateNetwork = true;
        ReadWritePaths = [ "/run/fp6-privacy" ];
        RestrictAddressFamilies = [ "AF_UNIX" ];
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
      };
    };

    systemd.tmpfiles.rules = [
      "d /run/fp6-privacy 2750 root users -"
      # Lock-screen emergency copy: the phone user writes, LightDM's greeter reads.
      "d /var/lib/fp6-privacy 2750 ${user} lightdm -"
    ];

    networking.networkmanager.wifi = lib.mkIf cfg.wifiMac {
      scanRandMacAddress = true;
      macAddress = "stable";
    };
    systemd.services.fp6-wifi-mac-migrate = lib.mkIf cfg.wifiMac {
      description = "Keep the MAC address of Wi-Fi networks saved before MAC randomisation (once)";
      wantedBy = [ "NetworkManager.service" ];
      before = [ "NetworkManager.service" ];
      unitConfig.ConditionPathExists = "!/var/lib/fp6-privacy-system/wifi-mac-migrated";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${package}/bin/fp6-privacy-system wifi-mac-migrate /etc/NetworkManager/system-connections /var/lib/fp6-privacy-system/wifi-mac-migrated";
      };
    };

    system.build.fp6-privacy-lock = { inherit package schema lssPlugin; inherit (pkgs.lomiri) lomiri lomiri-system-settings qtmir; };
  };
}
