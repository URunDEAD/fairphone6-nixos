{ config, lib, pkgs, ... }:
# FP6 suspend / power policy (Lomiri and Phosh).
#
# Evidence 2026-10-24 (kernel 7.2.0-fp6-nixos4, boot 46b7a26a, read-only):
# gsd-power (Phosh session) requested idle suspend twice (11:42:46, 12:18:37).
# Both times: "PM: suspend entry (deep)" -> "Wakeup pending. Abort CPU freeze"
# -> ufshcd-qcom 1d84000.ufshc "hibern8 exit failed 5" -> UFS host/device
# reset; systemd then retried s2idle with the same UFS failure. The system
# recovered, but a failed UFS resume is the same signature as the 2026-08-24
# dead-rootfs crash noted in the community charging-mode work. Until a UFS
# system-resume fix is proven, automatic suspend must not run.
#
# mode = "guarded" (default): suspend/hibernate disabled in systemd-sleep
#   (logind CanSuspend -> "na", so gsd-power, Lomiri and indicators cannot
#   suspend); gsd idle-suspend set to 'nothing'; UPower critical action
#   PowerOff; Lomiri battery percentage shown. Runtime PM is untouched.
# mode = "ufs-link-active-test": allows suspend for a supervised test only,
#   with battmgr power-supply wakeups disabled (PPS telemetry streams wake
#   events ~1/s) and UFS spm_lvl = ufsSpmLvl. Idle auto-suspend still stays
#   OFF; suspend is only entered by an explicit test command (tools/
#   feature-push/misc-suspend-test.sh, marked MANUAL). The mode name is
#   historical: 2026-10-26 analysis (results/feature-push-20261024/suspend/
#   README.md) showed genpd powers off ufs_mem_phy_gdsc/ufs_phy_gdsc in
#   suspend_noirq whatever spm_lvl says, so "link active" (2) cannot keep the
#   PHY alive; the default is now 5 (device POWERDOWN + link OFF, upstream
#   SM8550/SM8650 behaviour) and s2idle instead of deep.
#
# ufsLinkOffAtBoot (any mode): udev sets spm_lvl=5 on 1d84000.ufshc at add.
#   Kernel6 (7.2.0-fp6-nixos6) already defaults to 5 through the
#   "qcom,milos-ufshc" drvdata patch; the rule is a belt-and-braces for
#   kernels without it. It does not allow suspend by itself.
let
  cfg = config.services.fp6SuspendPolicy;
  guarded = cfg.mode == "guarded";
in {
  options.services.fp6SuspendPolicy = {
    enable = lib.mkEnableOption "FP6 suspend guard and battery power policy";
    mode = lib.mkOption {
      type = lib.types.enum [ "guarded" "ufs-link-active-test" ];
      default = "guarded";
    };
    wifiPowersave = lib.mkOption { type = lib.types.bool; default = true; description = "NetworkManager Wi-Fi powersave (ath11k)."; };
    ufsLinkOffAtBoot = lib.mkOption {
      type = lib.types.bool; default = false;
      description = "Set UFS spm_lvl=5 (device power-down, link off) at boot via udev.";
    };
    ufsSpmLvl = lib.mkOption {
      type = lib.types.enum [ 2 3 5 ]; default = 5;
      description = "UFS spm_lvl used by the ufs-link-active-test mode.";
    };
    memorySleepMode = lib.mkOption {
      type = lib.types.enum [ "s2idle" "deep" ]; default = "s2idle";
      description = "systemd-sleep MemorySleepMode in the test mode (s2idle first).";
    };
  };
  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      # Short power-key stays with the compositor (same as power-policy-fp6.nix).
      services.logind.settings.Login = {
        HandlePowerKey = lib.mkDefault "ignore";
        HandleSuspendKey = "ignore";
        HandleHibernateKey = "ignore";
        IdleAction = "ignore";
      };
      systemd.sleep.settings.Sleep = {
        AllowSuspend = if guarded then "no" else "yes";
        AllowHibernation = "no";
        AllowSuspendThenHibernate = "no";
        AllowHybridSleep = "no";
      } // lib.optionalAttrs (!guarded) { MemorySleepMode = cfg.memorySleepMode; };
      # Battery safety without suspend: power off at critical level.
      services.upower = {
        enable = true;
        usePercentageForPolicy = true;
        percentageLow = 15;
        percentageCritical = 5;
        percentageAction = 3;
        criticalPowerAction = "PowerOff";
      };
      # System dconf defaults (user overrides still win).
      programs.dconf.enable = true;
      programs.dconf.profiles.user.databases = [{
        settings = {
          "org/gnome/settings-daemon/plugins/power" = {
            sleep-inactive-battery-type = "nothing";
            sleep-inactive-ac-type = "nothing";
          };
          # Lomiri / Ayatana battery indicator: show the percentage in the panel.
          "org/ayatana/indicator/power" = { show-percentage = true; };
        };
      }];
      # Fewer dirty-page writeback wakeups (default 5 s).
      boot.kernel.sysctl."vm.dirty_writeback_centisecs" = 1500;
      networking.networkmanager.wifi.powersave = cfg.wifiPowersave;
    }
    (lib.mkIf (cfg.ufsLinkOffAtBoot && guarded) {
      services.udev.extraRules = ''
        # FP6: UFS PHY loses state when its GDSC is cut in system suspend;
        # power the link down (spm_lvl 5) instead of hibern8 (3).
        ACTION=="add", SUBSYSTEM=="platform", KERNEL=="1d84000.ufshc", ATTR{spm_lvl}="5"
      '';
    })
    (lib.mkIf (!guarded) {
      services.udev.extraRules = ''
        # FP6 TEST ONLY: UFS system-suspend level for the supervised test
        ACTION=="add", SUBSYSTEM=="platform", KERNEL=="1d84000.ufshc", ATTR{spm_lvl}="${toString cfg.ufsSpmLvl}"
        # battmgr psy events would wake s2idle/deep constantly while charging
        ACTION=="add", SUBSYSTEM=="power_supply", KERNEL=="qcom-battmgr-*", ATTR{power/wakeup}="disabled"
      '';
    })
  ]);
}
