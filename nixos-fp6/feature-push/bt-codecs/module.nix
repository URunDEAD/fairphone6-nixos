{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.bluetoothAudio;
  codecs = [ "sbc" "sbc_xq" "aac" "aac_eld" "aptx" "aptx_hd" "aptx_ll" "aptx_ll_duplex" "ldac" "lc3" "faststream" "faststream_duplex" "opus_05" "opus_05_51" "opus_05_71" "opus_05_duplex" "opus_05_pro" "opus_g" ];

in {
  options.hardware.fairphone6.bluetoothAudio = {
    enable = lib.mkEnableOption "per-device Bluetooth audio codec controls";
    package = lib.mkOption { type = lib.types.package; default = pkgs.callPackage ./package.nix {}; description = "Session audio helper."; };
    preferredCodecs = lib.mkOption {
      type = lib.types.listOf (lib.types.enum codecs);
      default = [ "ldac" "aptx_hd" "aac" "aptx" "sbc_xq" "sbc" ];
      description = "Music codec preference, highest first, used for new devices and the explicit Prefer quality action. Saved device profiles take precedence. This is not a codec allowlist.";
    };
    # null = leave the system's setting alone (network-fp6.nix turns hardware
    # volume on; WirePlumber 0.5 auto-switches by default), so enabling this
    # module never changes existing Bluetooth music or call audio.
    hardwareVolume = lib.mkOption { type = lib.types.nullOr lib.types.bool; default = null; description = "Headset hardware volume synchronization (bluez5.enable-hw-volume); null keeps the system setting."; };
    autoSwitchToHfp = lib.mkOption {
      type = lib.types.nullOr lib.types.bool; default = null;
      description = "null keeps WirePlumber's default. Let WirePlumber switch to HFP when a capture stream uses the Bluetooth microphone and restore music afterwards. Call software must route its microphone there; this is not a modem call-state listener.";
    };
    user = lib.mkOption { type = lib.types.str; default = "user"; description = "Existing Lomiri login user."; };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = cfg.preferredCodecs != [] && builtins.length cfg.preferredCodecs == builtins.length (lib.unique cfg.preferredCodecs); message = "Bluetooth audio preferredCodecs must be nonempty and unique."; }
    ];
    # The pinned Linux build already includes all requested codec dependencies.
    # Does not power the adapter, make it discoverable, or enable incoming pairing.
    services.pipewire.enable = true;
    services.pipewire.pulse.enable = true;
    services.pipewire.wireplumber.enable = true;
    services.pipewire.wireplumber.extraScripts = { "fp6-codec-order.lua" = builtins.replaceStrings [ "@ORDER@" ] [ (builtins.toJSON cfg.preferredCodecs) ] (builtins.readFile ./nix/fp6-codec-order.lua); };
    services.pipewire.wireplumber.extraConfig."53-fp6-bluetooth-audio" = {
      "wireplumber.components" = [{ name = "fp6-codec-order.lua"; type = "script/lua"; provides = "custom.fp6-codec-order"; }];
      # optional: a script problem is logged, WirePlumber (all audio) still starts.
      "wireplumber.profiles".main."custom.fp6-codec-order" = "optional";
    } // lib.optionalAttrs (cfg.hardwareVolume != null) {
      "monitor.bluez.properties" = { "bluez5.enable-hw-volume" = cfg.hardwareVolume; };
    } // lib.optionalAttrs (cfg.autoSwitchToHfp != null) {
      "wireplumber.settings"."bluetooth.autoswitch-to-headset-profile" = cfg.autoSwitchToHfp;
    };
    environment.systemPackages = [ cfg.package ];
    systemd.user.services.fp6-bluetooth-audio = {
      description = "Fairphone Bluetooth audio codec controls";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "pipewire-pulse.service" ];
      unitConfig.ConditionUser = cfg.user;
      environment.FP6_BT_PREFERRED_CODECS = lib.concatStringsSep "," cfg.preferredCodecs;
      serviceConfig = { Type = "dbus"; BusName = "com.fairphone6.BluetoothAudio1"; ExecStart = "${cfg.package}/bin/fp6-bluetooth-audio"; Restart = "on-failure"; RestartSec = 3; UMask = "0077"; NoNewPrivileges = true; };
    };
    system.build.fp6-bluetooth-audio = cfg.package;
  };
}
