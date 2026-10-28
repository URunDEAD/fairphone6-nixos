{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.localLlm;
in {
  options.hardware.fairphone6.localLlm = {
    enable = lib.mkEnableOption "on-device AI models and lazy loopback inference for the FP6 assistant";
    package = lib.mkOption { type = lib.types.package; default = import ./default.nix { inherit pkgs; }; description = "Rust manager and Qt 5 Lomiri settings page."; };
    cpuPackage = lib.mkOption { type = lib.types.package; default = import ./nix/llama.nix { inherit pkgs; kleidiAiSupport = cfg.enableKleidiAI; }; description = "CPU llama.cpp with FP6 ARMv8.6 dotprod/i8mm kernels."; };
    enableKleidiAI = lib.mkOption { type = lib.types.bool; default = false; description = "Build pinned KleidiAI microkernels; compare Q4_0 performance on the phone before enabling."; };
    enableVulkan = lib.mkOption { type = lib.types.bool; default = false; description = "Install experimental Vulkan llama-server. The user must separately opt in to GPU in Settings."; };
    vulkanPackage = lib.mkOption { type = lib.types.package; default = import ./nix/llama.nix { inherit pkgs; vulkanSupport = true; kleidiAiSupport = cfg.enableKleidiAI; }; description = "Optional Turnip Vulkan runtime."; };
    user = lib.mkOption { type = lib.types.str; default = "user"; description = "Existing phone login user owning model files and the session service."; };
    port = lib.mkOption { type = lib.types.port; default = 0; description = "Loopback proxy port, or zero for an ephemeral port exposed through D-Bus."; };
  };
  config = lib.mkIf cfg.enable {
    assertions = [ { assertion = builtins.hasAttr cfg.user config.users.users; message = "localLlm.user must name an existing user"; } ];
    environment.systemPackages = [ cfg.package cfg.cpuPackage ] ++ lib.optional cfg.enableVulkan (lib.lowPrio cfg.vulkanPackage);
    environment.pathsToLink = [ "/share/lomiri-system-settings" ];
    # System Settings can import the Rust QML bridge; Qt 5 phone package, never Qt 6 UITK.
    environment.sessionVariables.QML2_IMPORT_PATH = [ "${cfg.package}/${pkgs.qt5.qtbase.qtQmlPrefix}" ];
    services.dbus.packages = [ cfg.package ];
    systemd.user.services.fp6-llm = {
      description = "FP6 on-device model manager";
      after = [ "dbus.socket" ];
      unitConfig.ConditionUser = cfg.user;
      environment = {
        FP6_LLAMA_SERVER = "${cfg.cpuPackage}/bin/llama-server";
      } // lib.optionalAttrs cfg.enableVulkan {
        FP6_LLAMA_VULKAN_SERVER = "${cfg.vulkanPackage}/bin/llama-server";
      };
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.LocalLlm1";
        ExecStart = "${cfg.package}/bin/fp6-llm daemon --port ${toString cfg.port}";
        Restart = "on-failure";
        RestartSec = 3;
        UMask = "0077";
        KillMode = "control-group";
        TimeoutStopSec = 5;
      };
    };
  };
}
