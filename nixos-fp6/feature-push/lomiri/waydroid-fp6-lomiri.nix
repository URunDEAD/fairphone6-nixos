# Lomiri-compatible variant of ../../waydroid-fp6-production.nix (unchanged
# original asserts Phosh). Same matched Android Adreno 810 Mesa, same setup
# helper and same GPU health ExecStartPre; the container stays OFF at boot and
# the first initialization/start remains an explicit operator step.
# Only differences: the session user is a fixed "user" (Phosh's option is not
# owned by Lomiri) and the assertion requires the hardware-rendered Lomiri
# session instead of Phosh.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.waydroidGPU;
  workspace = ../..;
  mesa = (import (workspace + "/waydroid-a810") { inherit pkgs; }).runtime;
  sessionUser = config.hardware.fairphone6.lomiriGpu.sessionUser;
  setup = pkgs.writeShellApplication {
    name = "fp6-waydroid-setup";
    runtimeInputs = [ pkgs.python3 pkgs.coreutils config.virtualisation.waydroid.package ];
    text = ''
      exec python3 ${workspace + "/waydroid-a810/production/setup.py"} \
        --mesa ${mesa} \
        --session-user ${lib.escapeShellArg sessionUser} \
        --baseline-manifest ${workspace + "/waydroid-a810/trial/baseline/manifest.xml"} "$@"
    '';
  };
in {
  # Same option name as the production module so existing targets that set
  # hardware.fairphone6.waydroidGPU.enable = true keep working unchanged.
  options.hardware.fairphone6.waydroidGPU.enable = lib.mkEnableOption
    "persistent FP6 Waydroid with the matched Android Adreno810 Mesa libraries (Lomiri variant)";
  config = lib.mkIf cfg.enable {
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
        && config.services.desktopManager.lomiri.enable
        && !config.services.xserver.desktopManager.phosh.enable
        && !config.hardware.fairphone6.lomiriGpu.softwareRendering
        && config.systemd.services.waydroid-container.wantedBy == [];
      message = "Lomiri FP6 GPU Waydroid requires the ARM64 hardware-rendered Lomiri target with no container autostart.";
    }];
  };
}
