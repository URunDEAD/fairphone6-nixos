{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs {} }:
let
  evaluate = extra: import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = pkgs.stdenv.hostPlatform.system;
    modules = [ ../module.nix { system.stateVersion = "26.05"; } extra ];
  };
  off = (evaluate {}).config;
  on = (evaluate { hardware.fairphone6.bluetoothAudio = { enable = true; preferredCodecs = [ "aac" "sbc" ]; }; }).config;
  set = (evaluate { hardware.fairphone6.bluetoothAudio = { enable = true; hardwareVolume = false; autoSwitchToHfp = true; }; }).config;
  fragment = on.services.pipewire.wireplumber.extraConfig."53-fp6-bluetooth-audio";
  fragmentSet = set.services.pipewire.wireplumber.extraConfig."53-fp6-bluetooth-audio";
in assert !off.hardware.fairphone6.bluetoothAudio.enable;
assert !(off.systemd.user.services ? fp6-bluetooth-audio);
# Defaults leave hardware volume and HFP auto-switch to the rest of the system.
assert !(fragment ? "monitor.bluez.properties");
assert !(fragment ? "wireplumber.settings");
assert fragment."wireplumber.profiles".main."custom.fp6-codec-order" == "optional";
assert fragmentSet."monitor.bluez.properties"."bluez5.enable-hw-volume" == false;
assert fragmentSet."wireplumber.settings"."bluetooth.autoswitch-to-headset-profile" == true;
assert on.systemd.user.services.fp6-bluetooth-audio.environment.FP6_BT_PREFERRED_CODECS == "aac,sbc";
assert on.systemd.user.services.fp6-bluetooth-audio.unitConfig.ConditionUser == "user";
assert !(on.hardware.bluetooth.enable);
{ passed = true; }
