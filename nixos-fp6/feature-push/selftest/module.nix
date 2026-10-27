{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.selftest;
  home = config.users.users.${cfg.user}.home;
  configArg = lib.optionalString (cfg.probeFile != null) " --config ${lib.escapeShellArg (toString cfg.probeFile)}";
  # A system-only report and a session report both belong to the phone user.
  common = {
    Type = "oneshot";
    UMask = "0077";
    NoNewPrivileges = true;
    RestrictAddressFamilies = [ "AF_UNIX" ];
    ProtectSystem = "strict";
    ReadWritePaths = [ home ];
    PrivateTmp = true;
    TimeoutStartSec = 600;
    SuccessExitStatus = [ 1 ]; # WARN is a completed check; FAIL remains failed.
    StandardOutput = "journal";
    StandardError = "journal";
  };
in {
  options.hardware.fairphone6.selftest = {
    enable = lib.mkEnableOption "private post-update Fairphone self-tests";
    package = lib.mkOption { type = lib.types.package; default = import ./default.nix { inherit pkgs; }; description = "Self-test package including Qt 5 UI."; };
    user = lib.mkOption { type = lib.types.str; default = "phablet"; description = "Existing phone login user owning reports."; };
    probeFile = lib.mkOption { type = lib.types.nullOr lib.types.path; default = null; description = "Audited TOML probe list replacing the packaged list."; };
    sessionTarget = lib.mkOption { type = lib.types.str; default = "graphical-session.target"; description = "Target started by the phone shell session."; };
    automatic = lib.mkOption { type = lib.types.bool; default = true; description = "Run after boot and graphical session startup, including OTA reboots."; };
  };
  config = lib.mkIf cfg.enable {
    assertions = [{ assertion = builtins.hasAttr cfg.user config.users.users; message = "selftest.user must name an existing user"; }];
    environment.systemPackages = [ cfg.package ];
    environment.etc."fp6-selftest/probes.toml".source = if cfg.probeFile != null then cfg.probeFile else "${cfg.package}/share/fp6-selftest/probes.toml";
    environment.pathsToLink = [ "/share/lomiri-system-settings" ];
    xdg.mime.defaultApplications."x-scheme-handler/fp6-selftest" = "com.fairphone6.selftest.desktop";
    systemd.services.fp6-selftest-system = {
      description = "FP6 boot health check (system scope, no activation)";
      after = [ "multi-user.target" ];
      environment.HOME = home;
      serviceConfig = common // {
        User = cfg.user;
        ExecStart = "${cfg.package}/bin/fp6-selftest --system-only --quick --json${configArg}";
      };
    };
    systemd.timers.fp6-selftest-system = lib.mkIf cfg.automatic {
      wantedBy = [ "timers.target" ];
      timerConfig = { OnBootSec = "2min"; AccuracySec = "15s"; Unit = "fp6-selftest-system.service"; };
    };
    systemd.user.services.fp6-selftest = {
      description = "FP6 session health and feature self-test";
      after = [ cfg.sessionTarget ];
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = common // { ExecStart = "${cfg.package}/bin/fp6-selftest --json${configArg}"; };
      environment.HOME = home;
    };
    systemd.user.timers.fp6-selftest = lib.mkIf cfg.automatic {
      wantedBy = [ cfg.sessionTarget ];
      partOf = [ cfg.sessionTarget ];
      unitConfig.ConditionUser = cfg.user;
      timerConfig = { OnActiveSec = "90s"; AccuracySec = "15s"; Unit = "fp6-selftest.service"; };
    };
  };
}
