# Phosh session policy. CPU rendering stays the default for recovery; GPU
# rendering is an explicit separate target after clean-boot hardware checks.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.phosh;
  mesa = config.hardware.graphics.package;
  rendererEnv = if cfg.softwareRendering then {
    LIBGL_ALWAYS_SOFTWARE = "true";
    GBM_ALWAYS_SOFTWARE = "true";
    GALLIUM_DRIVER = "llvmpipe";
    MESA_LOADER_DRIVER_OVERRIDE = "kms_swrast";
    MESA_SHADER_CACHE_DISABLE = "true";
    LP_NUM_THREADS = "2";
    __EGL_VENDOR_LIBRARY_FILENAMES = "${mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
    LIBGL_DRIVERS_PATH = "${mesa}/lib/dri";
    GSK_RENDERER = "cairo";
  } else {
    GSK_RENDERER = "gl";
  };
in {
  options.hardware.fairphone6.phosh.softwareRendering = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Use the Phosh CPU renderer; disable only for a separate validated GPU trial.";
  };
  config = {
    services.desktopManager.lomiri.enable = lib.mkForce false;
    services.displayManager.defaultSession = lib.mkForce "phosh";
    services.xserver.displayManager.lightdm.enable = lib.mkForce false;
    services.xserver.displayManager.lightdm.extraSeatDefaults = lib.mkForce "";
    systemd.user.services.lomiri-keyboard.enable = lib.mkForce false;
    systemd.services.display-manager.enable = lib.mkForce false;

    services.xserver.desktopManager.phosh = {
      enable = true;
      user = "user";
      group = "users";
      phocConfig = {
        xwayland = "false";
        outputs.DSI-1 = { scale = 3; rotate = null; };
      };
    };
    systemd.services.phosh = {
      after = [ "iio-sensor-proxy.service" ];
      wants = [ "iio-sensor-proxy.service" ];
      conflicts = [ "display-manager.service" ];
      path = [ config.system.path ];
      environment = rendererEnv // {
        WLR_BACKENDS = "drm,libinput";
        WLR_DRM_DEVICES = "/dev/dri/card0";
        WLR_RENDERER = if cfg.softwareRendering then "pixman" else "gles2";
      };
      serviceConfig = {
        RestartSec = 5;
        TimeoutStopSec = 20;
      };
      startLimitIntervalSec = 60;
      startLimitBurst = 3;
    };

    # Remove global Lomiri-only overrides from PAM and newly launched apps.
    environment.sessionVariables = rendererEnv // lib.genAttrs [
      "GRID_UNIT_PX" "DEVICEINFO_DEVICE_PATH" "QT_PLUGIN_PATH"
      "MIR_SERVER_ADD_WAYLAND_EXTENSIONS"
    ] (_: lib.mkForce null);
    users.users.user.extraGroups = [ "feedbackd" ];
    # Let GTK use Wayland text-input with Stevia instead of a second IM daemon.
    i18n.inputMethod.enable = lib.mkForce false;
    # Make the OSK unit available to GNOME's systemd session explicitly.
    systemd.packages = [ pkgs.stevia ];
    environment.systemPackages = with pkgs; [
      phosh-mobile-settings gnome-console gnome-text-editor wlr-randr wayland-utils
    ];

    # Preserve the existing offline modem policy while the desktop changes.
    networking.modemmanager.enable = lib.mkForce false;
    services.geoclue2.enable = lib.mkForce false;
    assertions = [{
      assertion = !config.services.desktopManager.lomiri.enable
        && !config.services.xserver.displayManager.lightdm.enable
        && config.services.xserver.desktopManager.phosh.enable;
      message = "The Phosh trial must have a single compositor/session owner.";
    }];
  };
}
