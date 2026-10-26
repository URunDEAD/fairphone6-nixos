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
# Bridge frame (2026-10-26, see results/feature-push-20261024/boot-speed/):
# stage-1 paints the nixos-mobile theme's resting frame into fb0 right after
# the panel module loads (~1.3 s). It stays on the panel until plymouthd draws
# the same frame (DeviceScale=1, glow loop first), so there is no dark gap
# between the bootloader logo and Plymouth. The lvgui stage-1 splash would
# swap in the Mobile NixOS logo for ~2 s, so it is skipped by default
# (stage1Splash). The recovery menu and error screen are unaffected.
# The theme decodes frames lazily, so the first frame (boot and shutdown)
# no longer waits for every PNG to be decoded. plymouth-{poweroff,reboot,halt,kexec}
# start after user.slice, when the Lomiri session's Mir no longer holds the DRM master.
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

  # Stage-1 bridge frame: the nixos-mobile theme's resting frame (loop-0)
  # rendered at the same geometry as nixos-mobile.script computes it with
  # DeviceScale=1. Values are truncated like Math.Int:
  # SW = W*0.744, SH = SW*446/464, X = (W-SW)/2, Y = (H-SH)/2, on #05070D,
  # as raw XRGB8888 (bytes B,G,R,X) rows of fb0's line length
  # (stride, 4480 = 1120 px for the 1116 px panel).
  bridge = cfg.bridgeFrame;
  screenW = config.mobile.hardware.screen.width;
  screenH = config.mobile.hardware.screen.height;
  spriteW = screenW * 744 / 1000;
  spriteH = spriteW * 446 / 464;
  spriteX = (screenW - spriteW) / 2;
  spriteY = (screenH - spriteH) / 2;
  lineW = bridge.stride / 4;
  bridgeFrame = pkgs.runCommand "fp6-boot-bridge-frame.bgrx" {
    nativeBuildInputs = [ pkgs.buildPackages.imagemagick ];
  } ''
    magick -size ${toString lineW}x${toString screenH} xc:'#05070D' \
      \( ${./plymouth-theme-nixos-mobile/images/loop-0.png} -resize ${toString spriteW}x${toString spriteH}! \) \
      -geometry +${toString spriteX}+${toString spriteY} -composite \
      -alpha off -depth 8 bridge.png
    magick bridge.png -alpha opaque -depth 8 BGRA:$out
    size=$(stat -c %s $out)
    test "$size" = ${toString (bridge.stride * screenH)} || { echo "bridge frame is $size bytes" >&2; exit 1; }
  '';
  bridgeTask = pkgs.writeText "fp6-boot-splash-stage-1-bridge.rb"
    (builtins.replaceStrings
      [ "@width@" "@height@" "@stride@" "@stage1Splash@" ]
      [ (toString screenW) (toString screenH) (toString bridge.stride) (lib.boolToString cfg.stage1Splash) ]
      (builtins.readFile ./stage-1-bridge.rb));
  useBridge = quiet && bridge.enable && cfg.theme == "nixos-mobile";
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
      default = "nixos-mobile";
      description = ''
        Plymouth theme. "nixos-mobile" is the user-made boot/shutdown
        animation in ./plymouth-theme-nixos-mobile (NixOS logo spins in, glow
        loop until the greeter; outro plus CRT collapse on shutdown/reboot).
        Any theme shipped by pkgs.plymouth (e.g. "spinner") also works.
      '';
    };
    bridgeFrame = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Paint the nixos-mobile theme's resting frame into the msm fbdev
          (/dev/fb0) as soon as stage-1 has loaded the panel module. It stays on screen
          through stage-1, switch_root and early stage-2 until Plymouth draws
          the same frame. Plymouth then starts at the glow loop instead of the
          spin-in intro, so the logo does not disappear and reappear.
          Only used with theme "nixos-mobile" and without verbose.
        '';
      };
      stride = lib.mkOption {
        type = lib.types.ints.positive;
        default = 4480;
        description = "fb0 line length in bytes (/sys/class/graphics/fb0/stride). Stage-1 checks it before painting.";
      };
    };
    stage1Splash = lib.mkOption {
      type = lib.types.bool;
      default = !cfg.bridgeFrame.enable;
      defaultText = lib.literalExpression "!bridgeFrame.enable";
      description = ''
        Show the Mobile NixOS lvgui progress splash in stage-1. With the bridge
        frame it would replace the NixOS logo with the Mobile NixOS logo for
        ~2 s and then go back, so it is off by default then. The recovery menu
        (hold a volume key during boot) and the stage-1 error screen are
        separate applets and are not affected.
      '';
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
        themePackages = lib.optional (cfg.theme == "nixos-mobile")
          (pkgs.callPackage ./plymouth-theme-nixos-mobile { bootStartsAtLoop = useBridge; });
        # Native pixels (no automatic 2x on this ~430 dpi panel): the theme
        # scales its 464 px sprites straight to the screen width (sharper), and
        # the frame lands exactly where the stage-1 bridge frame put it.
        extraConfig = "DeviceScale=1";
      };
      # Shutdown/reboot animation: plymouth-{poweroff,reboot,halt,kexec} are
      # ordered after display-manager only, but the Lomiri session's Mir
      # (session scope in user.slice) can still hold the DRM master then, and
      # plymouthd cannot modeset. Start them only after user.slice has stopped.
      systemd.services.plymouth-poweroff.after = [ "user.slice" ];
      systemd.services.plymouth-reboot.after = [ "user.slice" ];
      systemd.services.plymouth-halt.after = [ "user.slice" ];
      systemd.services.plymouth-kexec.after = [ "user.slice" ];
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
    (lib.mkIf useBridge {
      mobile.boot.stage-1.tasks = [ bridgeTask ];
      mobile.boot.stage-1.contents = [
        { object = bridgeFrame; symlink = "/etc/fp6-bridge-frame.bgrx"; }
      ];
      system.build.fp6-boot-bridge-frame = bridgeFrame;
    })
  ]);
}
