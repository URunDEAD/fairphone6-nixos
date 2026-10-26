# Quiet boot + Plymouth splash + text-free greeter->session handover (2026-10-26).
# No-op unless `enable` is set. Notes: results/feature-push-20261024/boot-splash/README.md
#
# Boot (power-on -> greeter), observed on the live phone (kernel5 full target):
#  * 0-1.4 s bootloader; msm_dpu registers fb0 'msmdrmfb' at ~1.45 s and fbcon
#    takes VT1, so kernel + Mobile NixOS stage-1 log text scrolls there
#    (cmdline had loglevel=7 ignore_loglevel from nixos-fp6/configuration.nix).
#  * Stage-1 Tasks::Modules blocks ~9 s in the eswin touchscreen probe; the
#    Mobile NixOS stage-1 splash (boot/splash, lvgui) only starts after
#    Target<Graphics> (which waits for udev) at ~14.8 s and quits at the
#    switch_root ~16.1 s. It stays enabled (it also carries the recovery menu).
#  * Stage-2 systemd (~19 s) printed status to tty1 until the greeter (~26 s+).
#
# Fix: fbcon is not mapped to VT1 (fbcon=vc:2-6, as upstream
# mobile.beautification.silentBoot with useKernelLogo=false), so nothing ever
# draws text on the boot VT; quiet + loglevel=3 + systemd.show_status=false
# keep the console itself quiet (everything still goes to kmsg/journal); and
# Plymouth (DRM renderer on msm card0, DirtyFB-capable for the command-mode
# panel) shows a spinner from plymouth-start (sysinit) until LightDM takes
# over. LightDM has native Plymouth handover: for the wayland greeter it
# reuses Plymouth's active VT (tty1), runs `plymouth deactivate` (drops DRM
# master) and `plymouth quit --retain-splash` once the display server is up,
# so Mir gets the DRM master and the last splash frame stays until Mir draws.
# Like gdm.nix, display-manager conflicts with plymouth-quit.service (so
# multi-user.target does not quit Plymouth before LightDM) and lightdm.nix
# already has OnFailure=plymouth-quit.service.
#
# Login (greeter -> Lomiri session): LightDM runs the greeter on VT1 and the
# user session on the next free VT (VT2) and activates VT2 before the
# session's Mir owns the display; the greeter's Mir exits, the msm fbdev
# emulation is restored and VT2's fbcon is shown. logind (NAutoVTs=6) had
# just spawned autovt@tty2 = getty@tty2 for that VT switch (journal: "Started
# Getty on tty2" right after session 5 opened), so a login prompt was what
# flashed. NAutoVTs=0/ReserveVT=0 stop that and vt.global_cursor_default=0
# removes the cursor: VT2 is a plain black screen until Lomiri draws.
#
# verbose = true restores the original debug cmdline and drops Plymouth.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.bootSplash;
  quiet = cfg.enable && !cfg.verbose;
  # Debug parameters from nixos-fp6/configuration.nix (boot.kernelParams).
  # console=tty0 duplicates Mobile NixOS' console=tty1 (mobile.boot.defaultConsole).
  # printk.devkmsg=on is kept: it only stops kmsg rate limiting of userspace
  # (stage-1 log lines into the journal/ramoops) and prints nothing visible.
  debugParams = [ "console=tty0" "loglevel=7" "ignore_loglevel" ];
  # The Mobile NixOS stage-1 splash logo (artwork/logo/logo.white.svg), so
  # the stage-1 and Plymouth splash look the same.
  logo = pkgs.runCommand "fp6-boot-splash-logo.png" {
    nativeBuildInputs = [ pkgs.buildPackages.librsvg ];
  } ''
    rsvg-convert --width=${toString cfg.logoWidth} --keep-aspect-ratio \
      ${../../upstream/mobile-nixos/artwork/logo/logo.white.svg} -o $out
  '';
in
{
  options.hardware.fairphone6.bootSplash = {
    enable = lib.mkEnableOption "FP6 quiet boot with Plymouth splash and a text-free login transition";
    verbose = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Debug boot: keep the original verbose cmdline (console=tty0 loglevel=7
        ignore_loglevel), fbcon on VT1, systemd status output, and no Plymouth.
        The no-autovt login fix stays active.
      '';
    };
    theme = lib.mkOption {
      type = lib.types.str;
      default = "spinner";
      description = "Plymouth theme (a theme shipped by pkgs.plymouth).";
    };
    logoWidth = lib.mkOption {
      type = lib.types.ints.positive;
      default = 256;
      description = "Width in px of the Plymouth logo (Plymouth may scale it by the device scale).";
    };
  };

  # Second declaration of boot.kernelParams that only adds `apply`: the debug
  # parameters are plain (non-mkForce) list entries in configuration.nix, so
  # filtering the merged value is the only way to drop them without
  # restating every other parameter (psi=1, lsm=..., apparmor=1, ...).
  # The android boot image builders and the toplevel's kernel-params both
  # read config.boot.kernelParams, so they all see the filtered list.
  options.boot.kernelParams = lib.mkOption {
    apply = params: if quiet then lib.filter (p: !(lib.elem p debugParams)) params else params;
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      # No getty is spawned on the VT LightDM switches the session to (VT2).
      # The phone has no keyboard for VT logins; SSH/ADB stay the console.
      services.logind.settings.Login = {
        NAutoVTs = 0;
        ReserveVT = 0;
      };
    }
    (lib.mkIf quiet {
      # loglevel=3 (only errors and worse reach the console); also kernel.printk.
      boot.consoleLogLevel = 3;
      boot.kernelParams = [
        "quiet"
        # No fbcon on VT1 (boot, Plymouth and greeter VT); VT2-6 keep it.
        "fbcon=vc:2-6"
        "vt.global_cursor_default=0"
        "systemd.show_status=false"
        # The bootloader appends console=ttyHSL0 (no such device here).
        "plymouth.ignore-serial-consoles"
      ];
      boot.plymouth = {
        enable = true;
        theme = cfg.theme;
        logo = logo;
      };
      # gdm.nix pattern: LightDM quits Plymouth itself (deactivate + quit
      # --retain-splash after the greeter's display server is ready). Without
      # this, multi-user.target's plymouth-quit.service would end the splash
      # as soon as systemd-user-sessions is up, before the greeter.
      systemd.services.display-manager = {
        after = [ "plymouth-start.service" "plymouth-quit.service" ];
        conflicts = [ "plymouth-quit.service" ];
      };
      systemd.services.plymouth-quit.wantedBy = lib.mkForce [ ];
    })
  ]);
}
