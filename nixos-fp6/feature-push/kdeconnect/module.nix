{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.kdeconnect;
  package = cfg.package;
  interfaceArgs = lib.concatMapStringsSep " " (i: "--interface ${lib.escapeShellArg i}") cfg.wifiInterfaces;
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
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        ExecStart = "${package}/bin/lomiri-kdeconnectd ${interfaceArgs}";
        Restart = "on-failure";
        RestartSec = 5;
        UMask = "0077";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "full";
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" ];
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
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        ExecStart = "${package}/bin/lomiri-kdeconnect --clipboard-agent";
        Restart = "on-failure";
        RestartSec = 5;
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
