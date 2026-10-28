{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.kdeconnect;
  package = cfg.package;
  interfaceArgs = lib.concatMapStringsSep " " (i: "--interface ${lib.escapeShellArg i}") cfg.wifiInterfaces;
  # A failing daemon must not spin: back off 5 s -> 5 min and give up after
  # 10 starts in 30 min (r4 restarted it every 5 s, 548 times in one boot).
  restartPolicy = {
    Restart = "on-failure";
    RestartSec = 5;
    RestartSteps = 6;
    RestartMaxDelaySec = 300;
  };
  startLimit = { StartLimitIntervalSec = 1800; StartLimitBurst = 10; };
in {
  options.hardware.fairphone6.kdeconnect = {
    enable = lib.mkEnableOption "KDE Connect for Lomiri";
    package = lib.mkOption { type = lib.types.package; default = pkgs.callPackage ./package.nix {}; description = "Lomiri KDE Connect package."; };
    user = lib.mkOption { type = lib.types.str; description = "The existing phone login user who owns the service and paired devices."; };
    wifiInterfaces = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "wlan0" ];
      description = "Exact Wi-Fi interface names. No cellular, Ethernet, wildcard or global firewall rules are installed.";
    };
    allowUinput = lib.mkEnableOption "uinput access for the selected user (remote input still needs temporary per-device consent in the app)";
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = cfg.wifiInterfaces != [] && builtins.all (i: builtins.match "(wlan|wlp|wlx)[a-zA-Z0-9_.-]*" i != null) cfg.wifiInterfaces;
        message = "KDE Connect wifiInterfaces must be explicit wlan/wlp/wlx interface names."; }
      { assertion = builtins.hasAttr cfg.user config.users.users; message = "KDE Connect user must already exist."; }
    ];
    environment.systemPackages = [ package ];
    environment.pathsToLink = [ "/share/lomiri-content-hub/peers" ];
    networking.firewall.interfaces = lib.genAttrs cfg.wifiInterfaces (_: {
      allowedUDPPorts = [ 1716 ];
      allowedTCPPortRanges = [ { from = 1716; to = 1764; } ];
    });
    systemd.user.services.lomiri-kdeconnect = {
      description = "KDE Connect for Lomiri";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session-pre.target" ];
      unitConfig = { ConditionUser = cfg.user; } // startLimit;
      serviceConfig = restartPolicy // {
        ExecStart = "${package}/bin/lomiri-kdeconnectd ${interfaceArgs}";
        UMask = "0077";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "full";
        # AF_NETLINK: getifaddrs() (if-addrs) lists the Wi-Fi addresses over
        # rtnetlink; without it every start failed with EAFNOSUPPORT (r4).
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_NETLINK" ];
        LockPersonality = true;
        # DevicePolicy/PrivateDevices would deny the explicitly opted-in uinput.
      };
      preStart = ''
        mkdir -p "$HOME/.local/state/lomiri-kdeconnect" "$HOME/Downloads/KDE Connect"
        chmod 700 "$HOME/.local/state/lomiri-kdeconnect"
      '';
    };
    systemd.user.services.lomiri-kdeconnect-clipboard = {
      description = "Consented Lomiri clipboard bridge";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" "lomiri-kdeconnect.service" ];
      after = [ "lomiri-kdeconnect.service" ];
      unitConfig = { ConditionUser = cfg.user; } // startLimit;
      # The agent is a Qt GUI client (clipboard). graphical-session.target is
      # reached before Lomiri's Wayland socket exists (and before the session
      # imports WAYLAND_DISPLAY), so the first starts aborted in
      # QGuiApplication (xcb fallback / "Failed to create wl_display") and
      # left two SIGABRT cores per login (r5). Wait for the socket instead.
      environment = { WAYLAND_DISPLAY = "wayland-0"; QT_QPA_PLATFORM = "wayland"; };
      serviceConfig = restartPolicy // {
        ExecStartPre = "${pkgs.writeShellScript "kdeconnect-wait-wayland" ''
          socket="''${XDG_RUNTIME_DIR:?}/''${WAYLAND_DISPLAY:-wayland-0}"
          for _ in $(${pkgs.coreutils}/bin/seq 120); do
            [ -S "$socket" ] && exit 0
            ${pkgs.coreutils}/bin/sleep 0.5
          done
          echo "Wayland socket did not appear" >&2
          exit 1
        ''}";
        ExecStart = "${package}/bin/lomiri-kdeconnect --clipboard-agent";
        UMask = "0077";
        NoNewPrivileges = true;
      };
    };
    boot.kernelModules = lib.mkIf cfg.allowUinput [ "uinput" ];
    users.groups.kdeconnect-input = lib.mkIf cfg.allowUinput {};
    users.users.${cfg.user}.extraGroups = lib.mkIf cfg.allowUinput [ "kdeconnect-input" ];
    services.udev.extraRules = lib.mkIf cfg.allowUinput ''
      KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="kdeconnect-input", MODE="0660", OPTIONS+="static_node=uinput"
    '';
  };
}
