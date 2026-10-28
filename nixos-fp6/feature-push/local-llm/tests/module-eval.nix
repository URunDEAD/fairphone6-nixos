{ system ? "aarch64-linux", nixpkgs ? (import ../../../upstream/mobile-nixos/npins).nixpkgs }:
let
  eval = import (nixpkgs + "/nixos/lib/eval-config.nix") {
    inherit system;
    modules = [ ../module.nix {
      boot.isContainer = true;
      system.stateVersion = "26.05";
      users.users.user = { isNormalUser = true; home = "/home/user"; };
      hardware.fairphone6.localLlm = { enable = true; enableVulkan = true; };
    } ];
  };
in {
  service = eval.config.systemd.user.services.fp6-llm.serviceConfig;
  environment = eval.config.systemd.user.services.fp6-llm.environment;
  qml = eval.config.environment.sessionVariables.QML2_IMPORT_PATH;
  assertions = builtins.all (a: a.assertion) eval.config.assertions;
  toplevel = eval.config.system.build.toplevel.drvPath;
}
