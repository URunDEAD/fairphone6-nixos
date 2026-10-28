# App permissions (feature push 2026-10-24): "Apps that want to use the USB
# or NFC or such should also need to request so, like requesting permission
# for camera etc. Also allowing/restricting internet/network should be
# possible." Design, threat model limits and the phone checklist:
# results/feature-push-20261024/app-permissions/README.md
#
#  * fp6-permd (Rust, system service, root, system bus
#    org.fairphone.Permissions1): runtime permissions USB devices, NFC,
#    Bluetooth devices, contacts, calendar, call log & messages, files &
#    media, body & motion sensors. Prompt once (Allow / Allow only while
#    using the app / Don't allow), remembered per app, changeable and
#    revocable in Settings. Caller identity: AppArmor label (confined
#    apps), else the cgroup unit (lomiri-app-launch--...--<appid>--N.service
#    or app-*-<appid>-*.scope). Hands out USB device nodes as descriptors
#    (OpenDevice) and Bluetooth/NFC sockets (OpenSocket); daemons guarding a
#    resource ask on behalf of their callers (Authorize/AuthorizePid: the
#    gate an NFC daemon checks). Enforcement: AppArmor includes for the FP6
#    profiles (local/<profile>, reloaded at once), systemd drop-ins for
#    unconfined apps (RestrictAddressFamilies), and
#    /run/fp6-permissions/network.json for netpolicy.
#  * udev (deviceGate): no seat ACL ("uaccess") on USB device / USB HID /
#    USB serial nodes, so apps get them only through fp6-permd. MTP/PTP
#    devices (cameras, phones) keep the ACL for the file manager.
#  * Network: netpolicy (permissionsFile) drops the traffic of an app the
#    user denied Wi-Fi/other, mobile data, VPN or the local network, by its
#    cgroup, on every interface. Default: every app allowed.
#  * fp6-permission-agent (user service): prompts (Lomiri overlay, snap
#    decision fallback), focus for "while using", Settings data, camera /
#    location history from the privacy indicator.
#  * Lomiri (overlay lib.mkOrder 4200, after every other module's Lomiri
#    edits): Components/Fp6PermissionPrompt.qml in Shell.qml (anchored
#    insertion before the cursor-hiding timer, like charging feedback).
#  * System Settings > Security & Privacy > App permissions (AppAccess.qml
#    patched in the wrapper): per permission, per app (all permissions +
#    network toggles + reset), network access list, history.
#  * Camera / microphone / location (option media, default off): fp6-permd
#    decides (presets for the system apps, prompt Allow / Only this time /
#    Don't allow, Settings rows). Microphone and camera through PipeWire:
#    fp6-media-gate (user service) publishes per-client verdicts that
#    WirePlumber (wireplumber/fp6-media-permissions.lua) enforces when
#    linking; pipewire-pulse runs with pipewire-pulse-peer-pid.patch so
#    PulseAudio clients are known by their real pid. Camera device nodes:
#    AppArmor denial for confined apps the user refused. Location: GeoClue
#    with geoclue-fp6-app-identity.patch asks its agent (fp6-location-
#    indicator), which asks fp6-permd. Lomiri's trust-store is not used.
# System components keep their access: everything that is not an app unit
# (shell, telephony, MTP/usb-modes, KDE Connect, Quick Share, webshare,
# OTA, SSH sessions) and the apps in systemApps.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.appPermissions;
  pkg = pkgs.callPackage ./package.nix { };
  user = cfg.user;
  uid = config.users.users.${user}.uid;
  aa = config.hardware.fairphone6.apparmor or { enable = false; };
  # FP6 AppArmor profiles that are loaded (name = app id).
  profiles = lib.optionalAttrs (cfg.apparmor && (aa.enable or false))
    (lib.filterAttrs (n: p: p.state != "disable" && (aa.apps ? ${n})) config.security.apparmor.policies);
  stateDir = "/var/lib/fp6-permissions";
  media = cfg.media;
  mediaOn = cfg.enable && media.enable;
  mediaPerms = lib.optional media.camera "camera" ++ lib.optional media.microphone "microphone"
    ++ lib.optional media.location "location";
  pwGate = mediaOn && (media.camera || media.microphone);
  locationGate = mediaOn && media.location;
  pipewire = config.services.pipewire.package;
  pipewirePulsePatched = pipewire.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./pipewire/pipewire-pulse-peer-pid.patch ];
  });
  geocluePatched = config.services.geoclue2.package.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./geoclue/geoclue-fp6-app-identity.patch ];
  });
  # Apps GeoClue's configuration allowed without asking keep location.
  geoclueAllowed = lib.mapAttrs' (_: a: lib.nameValuePair a.desktopID { location = "allow"; })
    (lib.filterAttrs (_: a: a.isAllowed && !a.isSystem) (config.services.geoclue2.appConfig or { }));
  presets = lib.recursiveUpdate (lib.optionalAttrs media.location geoclueAllowed) media.defaults;
  extraTrusted = lib.optional pwGate "fp6-media-gate.service"
    ++ lib.optional locationGate "fp6-location-indicator.service";
  brokerConfig = (pkgs.formats.json { }).generate "fp6-permd.json" brokerSettings;
  brokerSettings = {
    inherit uid;
    system_apps = cfg.systemApps;
    trusted_units = lib.unique (cfg.trustedUnits ++ extraTrusted);
    agent_unit = "fp6-permission-agent.service";
    apparmor = lib.mapAttrs (_: p: { path = toString p.path; complain = p.state == "complain"; }) profiles;
    apparmor_parser = "${pkgs.apparmor-parser}/bin/apparmor_parser";
    state_dir = stateDir;
    run_dir = "/run/fp6-permissions";
    dropin_root = "/run/systemd/user";
    grace_secs = cfg.graceSeconds;
    prompt_timeout_secs = cfg.promptTimeout;
    devices = cfg.devices;
    network = cfg.network;
    enforce = true;
    media = lib.optionals mediaOn mediaPerms;
    defaults = lib.optionalAttrs mediaOn (lib.mapAttrs (_: lib.filterAttrs (p: _: lib.elem p mediaPerms)) presets);
  };
  busPolicy = pkgs.writeTextDir "share/dbus-1/system.d/org.fairphone.Permissions1.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
      "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <!-- Anyone may ask (apps, daemons); fp6-permd decides by the caller's
           AppArmor label / cgroup. Only root owns the name. -->
      <policy user="root">
        <allow own="org.fairphone.Permissions1"/>
        <!-- prompts to the phone user's agent -->
        <allow send_interface="org.fairphone.PermissionAgent1"/>
      </policy>
      <policy context="default">
        <allow send_destination="org.fairphone.Permissions1"/>
        <allow receive_sender="org.fairphone.Permissions1"/>
      </policy>
    </busconfig>
  '';
  udevRules = pkgs.writeTextDir "lib/udev/rules.d/72-fp6-app-permissions.rules" ''
    # FP6 app permissions: USB device nodes are not handed to the seat user
    # (no uaccess ACL, applied by 73-seat-late.rules); apps get them from
    # fp6-permd after the user allowed it. Cameras/phones (MTP/PTP) keep the
    # ACL for the file manager; nothing here touches the phone's own gadget
    # (USB network, MTP, ADB) or internal devices.
    SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ENV{ID_MTP_DEVICE}!="1", ENV{ID_GPHOTO2}!="1", TAG-="uaccess"
    SUBSYSTEM=="hidraw", SUBSYSTEMS=="usb", TAG-="uaccess"
    SUBSYSTEM=="tty", KERNEL=="ttyACM[0-9]*|ttyUSB[0-9]*", TAG-="uaccess"
  '';
  # Shell.qml: before the stock cursor-hiding timer (an anchor; composes
  # with charging feedback 3900 and lock sounds 3800).
  shellAnchor = "    Timer {\n        id: cursorHidingTimer\n";
  shellInsert = lib.concatStringsSep "\n" [
    "    // FP6 app permissions (nixos-fp6/feature-push/app-permissions): the"
    "    // runtime-permission prompt (fp6-permission-agent) and the focused-app"
    "    // hook for \"Allow only while using the app\". Hidden while locked."
    "    Fp6PermissionPrompt {"
    "        id: fp6PermissionPrompt"
    "        anchors.fill: parent"
    "        z: cursor.z + 0.5   // below the display-off input block (privacy-lock)"
    "        locked: shell.showingGreeter"
    "        focusedAppId: shell.focusedApplicationId"
    "    }"
    "" ""
  ];
in {
  options.hardware.fairphone6.appPermissions = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Runtime app permissions (USB devices, NFC, Bluetooth devices,
        contacts, calendar, call log & messages, files, sensors) with prompts,
        and the per-app network permission. Settings > Security & Privacy >
        App permissions.
      '';
    };
    user = lib.mkOption {
      type = lib.types.str;
      default = config.services.fp6LomiriSensors.user or "user";
      defaultText = lib.literalExpression ''config.services.fp6LomiriSensors.user or "user"'';
      description = "The phone user (answers prompts, owns the decisions).";
    };
    prompts = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "The Lomiri prompt overlay (Shell.qml patch); off = snap-decision notifications only.";
    };
    devices = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "fp6-permd hands out USB device nodes and Bluetooth/NFC sockets after consent.";
    };
    deviceGate = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        No uaccess ACL on USB device, USB HID and USB serial nodes (udev), so
        apps need fp6-permd for them. Off: the seat user (every app) keeps
        udev's default ACLs (e.g. FIDO keys in Firefox) and the USB
        permission only gates broker-aware apps.
      '';
    };
    network = lib.mkOption {
      type = lib.types.bool;
      default = config.hardware.fairphone6.netpolicy.enable or false;
      defaultText = lib.literalExpression "config.hardware.fairphone6.netpolicy.enable";
      description = ''
        Per-app network permission through netpolicy. Every app keeps its
        network until the user turns something off for it.
      '';
    };
    apparmor = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Runtime includes for the FP6 AppArmor profiles (confined apps: grants for handed-out descriptors, denied data paths).";
    };
    systemApps = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "lomiri-system-settings" "lomiri-dialer-app" "lomiri-messaging-app" "lomiri-addressbook-app"
        "lomiri-terminal-app"
        "fp6-usb-drive" "fp6-file-picker" "fp6-webshare" "fp6-bugreport" "fp6-dictation" "fp6-captions"
        "com.fairphone6.KdeConnect" "com.fairphone6.QuickShare" "com.fairphone6.Share" "com.fairphone6.selftest"
        "org.fairphone.netpolicy"
      ];
      description = ''
        App ids (desktop ids) that are system components: never prompted,
        network never removable. Everything that is not an app (shell,
        services, SSH sessions) is a system component anyway. The terminal
        is the owner's shell (like an SSH session).
      '';
    };
    trustedUnits = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "fp6-nfcd.service" ];
      description = "User units (of the phone user) that may ask fp6-permd on behalf of their callers (Authorize/AuthorizePid). Root and system users always may.";
    };
    graceSeconds = lib.mkOption {
      type = lib.types.ints.between 0 3600;
      default = 30;
      description = "\"Allow only while using\": the app keeps access this long after it lost focus; open USB devices are taken back after that.";
    };
    media = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Camera, microphone and location permissions per app (kill switch,
          off by default): prompts (Allow / Only this time / Don't allow),
          Settings rows, enforcement through PipeWire/WirePlumber
          (fp6-media-gate), AppArmor (camera nodes of confined apps) and
          GeoClue's agent. Off: everything as before (no gate, stock
          pipewire-pulse and GeoClue).
        '';
      };
      camera = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Gate PipeWire video capture (and the camera nodes of confined apps).";
      };
      microphone = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Gate PipeWire audio capture from microphones.";
      };
      location = lib.mkOption {
        type = lib.types.bool;
        default = (config.services.fp6Cellular.gnss.enable or false) && config.services.geoclue2.enable;
        defaultText = lib.literalExpression "config.services.fp6Cellular.gnss.enable && config.services.geoclue2.enable";
        description = "Gate GeoClue per app (needs the fp6-location-indicator agent of the cellular module).";
      };
      defaults = lib.mkOption {
        type = lib.types.attrsOf (lib.types.attrsOf (lib.types.enum [ "allow" "while_using" "ask" "deny" ]));
        default = {
          lomiri-camera-app = { camera = "allow"; microphone = "allow"; location = "while_using"; };
          lomiri-camera-app-secure = { camera = "allow"; microphone = "allow"; };
          lomiri-barcode-reader-app = { camera = "while_using"; };
          fp6-wifi-qr-scan = { camera = "while_using"; };
          fp6-magnifier = { camera = "while_using"; };
          fairphone-ocr = { camera = "while_using"; };
          lomiri-weather-app = { location = "while_using"; };
          "io.github.rinigus.PureMaps" = { location = "while_using"; };
          fp6-gps = { location = "while_using"; };
          fp6-speed = { location = "while_using"; };
          fp6-altimeter = { location = "while_using"; };
          fp6-compass = { location = "while_using"; };
          fp6-sound = { microphone = "while_using"; };
          fp6-tuner = { microphone = "while_using"; };
          fp6-callrec = { microphone = "allow"; };
        };
        description = ''
          Starting decisions per app id (camera / microphone / location) until
          the user changes them; Reset in Settings goes back to these. Apps
          GeoClue's configuration allowed (services.geoclue2.appConfig) start
          with location allowed. System apps (systemApps: Phone, Messaging,
          Settings, dictation, captions, ...) and services are never asked.
        '';
      };
    };
    promptTimeout = lib.mkOption {
      type = lib.types.ints.between 5 600;
      default = 60;
      description = "Unanswered prompts are refused (and asked again next time) after this many seconds.";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [ {
    assertions = [
      { assertion = media.enable -> mediaPerms != [ ];
        message = "appPermissions.media: enable at least one of camera, microphone, location."; }
      { assertion = pwGate -> (config.services.pipewire.enable && config.services.pipewire.wireplumber.enable && config.services.pipewire.pulse.enable);
        message = "appPermissions.media camera/microphone need PipeWire with WirePlumber and pipewire-pulse."; }
      { assertion = locationGate -> (config.services.geoclue2.enable && (config.services.fp6Cellular.gnss.enable or false));
        message = "appPermissions.media.location needs GeoClue and the fp6-location-indicator agent (services.fp6Cellular.gnss)."; }
      { assertion = config.users.users ? ${user} && uid != null && uid >= 1000;
        message = "appPermissions.user must be an existing normal user with a fixed uid."; }
      { assertion = cfg.network -> (config.hardware.fairphone6.netpolicy.enable or false);
        message = "appPermissions.network enforces through netpolicy (hardware.fairphone6.netpolicy.enable)."; }
      { assertion = cfg.network -> (config.hardware.fairphone6.netpolicy.uid or uid) == uid;
        message = "appPermissions and netpolicy must be for the same user."; }
    ];

    hardware.fairphone6.netpolicy.permissionsFile = lib.mkIf cfg.network "/run/fp6-permissions/network.json";

    # Portal access dialogs (camera/location/... portals) through the same
    # prompt: xdg-desktop-portal-lomiri's Access backend asks the agent.
    xdg.portal.config = lib.mkIf (config.hardware.fairphone6.filePickerPortal.enable or false) (let
      route = { "org.freedesktop.impl.portal.Access" = [ "lomiri" ]; };
    in { common = route; lomiri = route; });

    environment.systemPackages = [ pkg.core ];
    services.dbus.packages = [ busPolicy ];
    services.udev.packages = lib.optional cfg.deviceGate udevRules;

    # The broker writes drop-ins here (root-owned, read by user managers).
    systemd.tmpfiles.rules = [ "d /run/systemd/user 0755 root root -" ];

    # Confined apps: each FP6 profile's `include if exists <local/NAME>`
    # pulls in the broker's generated file.
    security.apparmor.includes = lib.mapAttrs' (n: _: lib.nameValuePair "local/${n}" ''
      include if exists "${stateDir}/apparmor/${n}"
    '') profiles;

    systemd.services.fp6-permd = {
      description = "FP6 app permission broker (runtime permissions, device access, app network rules)";
      wantedBy = [ "multi-user.target" ];
      after = [ "dbus.service" "apparmor.service" "systemd-tmpfiles-setup.service" ];
      before = lib.optional cfg.network "fp6-netpolicy-helper.service";
      requires = [ "dbus.service" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "org.fairphone.Permissions1";
        ExecStart = "${pkg.core}/bin/fp6-permd ${brokerConfig}";
        Restart = "on-failure";
        RestartSec = 2;
        StateDirectory = "fp6-permissions";
        StateDirectoryMode = "0755";
        RuntimeDirectory = "fp6-permissions";
        RuntimeDirectoryMode = "0755";
        RuntimeDirectoryPreserve = "yes";
        UMask = "0022";
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ReadWritePaths = [ "/run/systemd/user" "-/var/cache/apparmor" ];
        # Device nodes and sysfs (USB "authorized" for revocation) stay
        # reachable: no PrivateDevices / ProtectKernelTunables.
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_BLUETOOTH" "AF_NFC" ];
        CapabilityBoundingSet = [ "CAP_MAC_ADMIN" ];
        SystemCallArchitectures = "native";
      };
    };

    systemd.user.services.fp6-permission-agent = {
      description = "FP6 app permission prompts and settings (session agent)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = user;
      path = [ config.systemd.package ];
      serviceConfig = {
        ExecStart = "${pkg.core}/bin/fp6-permission-agent";
        Restart = "on-failure";
        RestartSec = 2;
      };
    };

    nixpkgs.overlays = lib.mkOrder 4200 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = if !cfg.prompts then lprev.lomiri else lprev.lomiri.overrideAttrs (old: {
          postPatch = (old.postPatch or "") + ''
            # FP6 app permissions (after every other module's Lomiri edits)
            install -m644 ${./qml/Fp6PermissionPrompt.qml} qml/Components/Fp6PermissionPrompt.qml
            substituteInPlace qml/Shell.qml \
              --replace-fail ${lib.escapeShellArg shellAnchor} ${lib.escapeShellArg (shellInsert + shellAnchor)}
            grep -q 'Fp6PermissionPrompt {' qml/Shell.qml
            grep -q 'readonly property string focusedApplicationId' qml/Shell.qml
          '';
          passthru = (old.passthru or { }) // { fp6AppPermissions = true; };
        });
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        })).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.buildPackages.patch ];
          postInstall = (old.postInstall or "") + ''
            # Security & Privacy > App permissions: the FP6 section on top.
            sp=$out/share/lomiri-system-settings/qml-plugins/security-privacy
            test -e "$sp/AppAccess.qml"
            cp --remove-destination "$(readlink -f "$sp/AppAccess.qml")" "$sp/AppAccess.qml"
            chmod u+w "$sp/AppAccess.qml"
            patch -d "$sp" -p1 --forward --batch < ${./settings-page/lss-app-access.patch}
            grep -q Fp6PermissionsSection "$sp/AppAccess.qml"
            test -e $out/share/lomiri-system-settings/qml-plugins/fp6-app-permissions/Fp6PermissionsSection.qml
          '';
        });
      });
    }) ];

    # Every loaded FP6 profile must still compile with the largest generated
    # includes (all granted / all denied), as apparmor.service would.
    system.checks = lib.optional (profiles != { }) (pkgs.runCommand "fp6-app-permissions-apparmor-compile" { } ''
      set -eu
      for kind in allow deny; do
        inc=$TMPDIR/inc-$kind
        mkdir -p $inc/local
        ${lib.concatStrings (lib.mapAttrsToList (n: _: ''
          cp ${pkg.core}/share/fp6-permissions/sample-$kind.inc $inc/local/${n}
        '') profiles)}
        ${lib.concatStrings (lib.mapAttrsToList (n: p: ''
          echo "compile ${n} with the $kind include"
          ${pkgs.apparmor-parser}/bin/apparmor_parser --config-file /dev/null -Q -K \
            -I $inc -I ${config.environment.etc."apparmor.d".source} \
            ${lib.concatMapStringsSep " " (x: "-I ${x}/etc/apparmor.d") config.security.apparmor.packages} \
            ${p.path} 2>&1 | { grep -v 'Cache read/write disabled' || true; }
          test "''${PIPESTATUS[0]}" = 0
        '') profiles)}
      done
      touch $out
    '');

    system.build.fp6-app-permissions = {
      inherit (pkg) core lssPlugin;
      inherit brokerConfig brokerSettings udevRules;
      lomiri = pkgs.lomiri.lomiri;
      lomiri-system-settings = pkgs.lomiri.lomiri-system-settings;
    };
  }

  (lib.mkIf pwGate {
    # WirePlumber enforces the gate's verdicts when linking.
    services.pipewire.wireplumber.extraScripts."fp6-media-permissions.lua" = builtins.readFile ./wireplumber/fp6-media-permissions.lua;
    services.pipewire.wireplumber.extraConfig."60-fp6-media-permissions" = {
      "wireplumber.components" = [ { name = "fp6-media-permissions.lua"; type = "script/lua"; provides = "custom.fp6-media-permissions"; } ];
      # optional: a script problem is logged, audio keeps working (ungated)
      "wireplumber.profiles".main."custom.fp6-media-permissions" = "optional";
    };
    # PulseAudio clients by their real pid (SO_PEERCRED): only the pulse
    # server runs the patched build, libraries and other users stay stock.
    systemd.user.services.pipewire-pulse.serviceConfig.ExecStart = [ "" "${pipewirePulsePatched}/bin/pipewire-pulse" ];
    systemd.user.services.fp6-media-gate = {
      description = "FP6 camera and microphone permissions for PipeWire clients";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "wireplumber.service" "pipewire.service" ];
      wants = [ "wireplumber.service" ];
      unitConfig = { ConditionUser = user; StartLimitIntervalSec = 0; };
      environment = {
        FP6_PW_DUMP = "${pipewire}/bin/pw-dump";
        FP6_PW_METADATA = "${pipewire}/bin/pw-metadata";
        FP6_MEDIA_PERMS = lib.concatStringsSep " " (lib.filter (p: p != "location") mediaPerms);
      };
      serviceConfig = {
        ExecStart = "${pkg.core}/bin/fp6-media-gate";
        Restart = "always";
        RestartSec = 3;
      };
    };
  })

  (lib.mkIf locationGate {
    # GeoClue names host apps by AppArmor profile / app cgroup and asks the
    # agent about them; the agent asks fp6-permd.
    systemd.services.geoclue.serviceConfig.ExecStart = [ "" "${geocluePatched}/libexec/geoclue" ];
    systemd.user.services.fp6-location-indicator.environment.FP6_LOCATION_PERMISSIONS = "1";
  })

  (lib.mkIf mediaOn {
    system.build.fp6-app-permissions-media = {
      inherit pipewirePulsePatched geocluePatched;
      inherit (pkg) core;
      config = brokerConfig;
    };
  })
  ]);
}
