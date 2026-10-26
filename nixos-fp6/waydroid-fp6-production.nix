# Persistent Waydroid integration, separate from the bounded diagnostic trials.
# Enabling installs the service but leaves its first initialization/start explicit.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.waydroidGPU;
  mesa = (import ./waydroid-a810 { inherit pkgs; }).runtime;
  setup = pkgs.writeShellApplication {
    name = "fp6-waydroid-setup";
    runtimeInputs = [ pkgs.python3 pkgs.coreutils config.virtualisation.waydroid.package ];
    text = ''
      exec python3 ${./waydroid-a810/production/setup.py} \
        --mesa ${mesa} \
        --session-user ${lib.escapeShellArg config.services.xserver.desktopManager.phosh.user} \
        --baseline-manifest ${./waydroid-a810/trial/baseline/manifest.xml} "$@"
    '';
  };
in {
  options.hardware.fairphone6.waydroidGPU.enable = lib.mkEnableOption
    "persistent FP6 Waydroid with the matched Android Adreno810 Mesa libraries";
  config = lib.mkIf cfg.enable {
    # The diagnostic parent deliberately force-disables the stock module.
    # This distinct target selects the persistent implementation instead.
    hardware.fairphone6.androidMesaTrial.enable = lib.mkForce false;
    virtualisation.waydroid.enable = lib.mkOverride 40 true;
    environment.systemPackages = [ setup ];
    environment.etc."fp6-waydroid-gpu-mesa".source = mesa;
    systemd.tmpfiles.rules = [
      "d /etc/waydroid-extra 0755 root root -"
      "L /etc/waydroid-extra/images - - - - /var/lib/fp6-waydroid-images"
    ];
    systemd.services.waydroid-container = {
      wantedBy = lib.mkForce [];
      unitConfig.ConditionPathExists = "/var/lib/waydroid/fp6-gpu-ready.json";
      serviceConfig.ExecStartPre = pkgs.writeShellScript "fp6-waydroid-gpu-check" ''
        set -eu
        test -c /dev/dri/renderD128
        test -c /dev/dma_heap/system
        case "$(cat /sys/bus/platform/devices/3d00000.gpu/power/runtime_status)" in
          active|suspended) ;;
          *) echo 'FP6 GPU is unhealthy; refusing to start Android' >&2; exit 1 ;;
        esac
      '';
    };
    assertions = [{
      assertion = pkgs.stdenv.hostPlatform.system == "aarch64-linux"
        && config.services.xserver.desktopManager.phosh.enable
        && !config.hardware.fairphone6.phosh.softwareRendering;
      message = "Persistent FP6 GPU Waydroid requires the ARM64 hardware-rendered Phosh target.";
    }];
  };
}
