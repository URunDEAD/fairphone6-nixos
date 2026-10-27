{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.ocr;
in {
  options.hardware.fairphone6.ocr = {
    enable = lib.mkEnableOption "offline OCR and the Lomiri text recognition app";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "OCR package to install.";
    };
    screenshotDirectory = lib.mkOption {
      type = lib.types.str;
      default = "%h/Pictures/Screenshots";
      description = "Screenshot directory; systemd expands %h to the user's home. No recursive scan.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.pathsToLink = [ "/share/lomiri-content-hub" ];
    environment.systemPackages = [ cfg.package pkgs.lomiri.lomiri-content-hub ];
    services.dbus.packages = [ cfg.package pkgs.lomiri.lomiri-content-hub ];
    systemd.user.services.fairphone-ocr = {
      description = "Offline image text recognition";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Ocr1";
        ExecStart = "${cfg.package}/bin/fairphone-ocr serve --screenshot-dir ${lib.escapeShellArg cfg.screenshotDirectory}";
        Restart = "on-failure";
        RestartSec = 3;
        UMask = "0077";
        Nice = 10;
        CPUQuota = "100%";
        MemoryMax = "384M";
        NoNewPrivileges = true;
        RuntimeDirectory = "fairphone-ocr";
        RuntimeDirectoryMode = "0700";
        Environment = "TMPDIR=%t/fairphone-ocr";
        RestrictAddressFamilies = [ "AF_UNIX" ];
      };
    };
  };
}
