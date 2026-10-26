{ config, lib, pkgs, ... }:

let
  # These two files were copied unchanged from this phone's postmarketOS root.
  # Keep the stock tree as well: its lookup code and shared includes are needed.
  ucmPackage = pkgs.alsa-ucm-conf.overrideAttrs (old: {
    pname = "alsa-ucm-conf-fp6";
    postInstall = (old.postInstall or "") + ''
      install -Dm644 "${./audio-fp6-ucm}/Fairphone/fp6/HiFi.conf" \
        "$out/share/alsa/ucm2/Fairphone/fp6/HiFi.conf"
      install -Dm644 "${./audio-fp6-ucm}/conf.d/milos/Fairphone (Gen. 6).conf" \
        "$out/share/alsa/ucm2/conf.d/milos/Fairphone (Gen. 6).conf"
    '';
  });
  ucmPath = "${ucmPackage}/share/alsa/ucm2";
  audioServices = lib.genAttrs [ "pipewire" "pipewire-pulse" "wireplumber" ] (_: {
    environment.ALSA_CONFIG_UCM2 = ucmPath;
  });
in
{
  # Import this module to install the FP6 routing; it does not run mixer commands
  # during activation. UCM applies its sequences when the card/profile is used.
  services.pulseaudio.enable = lib.mkDefault false;
  security.rtkit.enable = lib.mkDefault true;
  services.pipewire = {
    enable = lib.mkDefault true;
    alsa.enable = lib.mkDefault true;
    pulse.enable = lib.mkDefault true;
    wireplumber.enable = lib.mkDefault true;
    wireplumber.extraConfig."51-fp6-alsa" = {
      "monitor.alsa.rules" = [{
        # Preserve the original Qualcomm buffering settings, scoped to the
        # internal sound card so USB/Bluetooth devices keep their own defaults.
        matches = [
          { "node.name" = "~alsa_input.platform-sound.*"; }
          { "node.name" = "~alsa_output.platform-sound.*"; }
        ];
        actions.update-props = {
          "audio.format" = "S16LE";
          "audio.rate" = 48000;
          "api.alsa.period-size" = 4096;
          "api.alsa.period-num" = 6;
          "api.alsa.headroom" = 512;
        };
      }];
    };
  };

  environment.variables.ALSA_CONFIG_UCM2 = ucmPath;
  environment.systemPackages = [ pkgs.alsa-utils ];
  systemd.user.services = lib.mkIf (!config.services.pipewire.systemWide) audioServices;
  systemd.services = lib.mkIf config.services.pipewire.systemWide audioServices;
}
