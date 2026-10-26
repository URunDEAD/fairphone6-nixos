{ config, lib, ... }:
let cfg = config.services.fp6PhonePowerPolicy;
in {
  options.services.fp6PhonePowerPolicy.enable = lib.mkEnableOption "ignore logind's FP6 short power-key press until compositor screen-power integration is ready";
  config = lib.mkIf cfg.enable {
    # Only change the short-press action. No idle/suspend/lid/long-press
    # policy is changed, and this does not implement screen-off or wake.
    services.logind.settings.Login.HandlePowerKey = "ignore";
  };
}
