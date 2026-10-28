# Lock and unlock sounds (feature push 2026-10-28). Notes, licences and the
# phone checklist: results/feature-push-20261024/lock-sounds/README.md.
#
#  * Lomiri (overlay lib.mkOrder 3800, postPatch after every other module's
#    Lomiri edits): qml/Components/Fp6LockSounds.qml, instantiated in
#    Shell.qml before the cursor-hiding timer with greeter.active, the call
#    state and wizard/greeter-mode/secondary-screen suppression. Lock = the
#    display turned off by the power key or the timeout while unlocked (not
#    proximity), or the lock screen appearing with the display on; unlock =
#    greeter.active going false with the display on. Never with Lock sounds
#    off, in Silent Mode (indicator-sound silent-mode), with Do Not Disturb
#    on (com.fairphone6.notifications dnd-active), during calls, nor for the
#    session-start lock screen. Fp6LockSoundPlayer.qml: SoundEffect (WAV
#    decoded once, PulseAudio media.role "event") loaded only while the
#    display is on and that sound can come next; Audio fallback for files
#    that are not prepared.
#  * Settings > Sound > Other sounds: "Lock sounds" switch (default on),
#    Lock sound / Unlock sound pickers (None, 9 bundled sounds, Custom... from
#    ~/Music, ~/Ringtones, imported ringtones; Preview). Replaces the stock
#    page's unwired placeholder "Lock sound" switch. The wrapper's copy of
#    the Sound page is edited in postInstall, after audio-routing's patch.
#  * fp6-lock-sounds (Rust): converts the bundled sounds at build time
#    (trim, peak -9 dBFS, 16-bit WAV); as a user service (path unit on the
#    dconf database) prepares custom files into ~/.cache/fp6-lock-sounds and
#    lists them in prepared-files.
#  * GSettings com.fairphone6.lock-sounds: enabled (b), lock-sound,
#    unlock-sound (s: id, absolute path or ""), prepared-files (as).
#
# Overlay order: lib.mkOrder 3800 (used so far: 2000-3700; charging
# animation/sound is 3900). Shell.qml is edited by an anchor
# (substituteInPlace on the stock cursorHidingTimer), not a context patch.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lockSounds;
  home = config.users.users.${cfg.user}.home or "/home/${cfg.user}";
  pkg = pkgs.callPackage ./package.nix { inherit home; };
  withDnd = config.hardware.fairphone6.notifications.enable or false;
in {
  options.hardware.fairphone6.lockSounds = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Lock/unlock sounds in Lomiri with Settings > Sound > Lock sounds
        (on by default once installed; bundled sounds or a custom file).
      '';
    };
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "The phone user (home folder for Custom..., user service).";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 3800 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          buildInputs = (old.buildInputs or [ ]) ++ [ pkg.schema ];
          postPatch = (old.postPatch or "") + pkg.lomiriPostPatch { inherit withDnd; };
          passthru = (old.passthru or { }) // { fp6LockSounds = true; };
        });
        # The wrapper merges the unwrapped tree with lndir: edit only its
        # copy of the Sound page (after audio-routing's patch).
        lomiri-system-settings = lprev.lomiri-system-settings.overrideAttrs (old: {
          postInstall = (old.postInstall or "") + pkg.lssPostInstall;
          # glib's hook puts the schema on the wrapper's XDG_DATA_DIRS.
          buildInputs = (old.buildInputs or [ ]) ++ [ pkg.schema ];
          passthru = (old.passthru or { }) // { fp6LockSounds = true; };
        });
      });
    }) ];

    environment.systemPackages = [ pkg.schema pkg.tool ];

    # Custom sounds: prepared when the dconf database changes (the Settings
    # pickers write there) and once per session.
    systemd.user.services.fp6-lock-sounds = {
      description = "FP6 lock sounds: prepare custom sounds";
      wantedBy = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${lib.getExe pkg.tool} sync";
        Nice = 10;
      };
    };
    systemd.user.paths.fp6-lock-sounds = {
      description = "FP6 lock sounds: watch the settings";
      wantedBy = [ "graphical-session.target" ];
      unitConfig.ConditionUser = cfg.user;
      pathConfig = {
        PathChanged = "%h/.config/dconf/user";
        Unit = "fp6-lock-sounds.service";
      };
    };

    system.build.fp6-lock-sounds = {
      inherit (pkg) tool schema sounds qml lssPage;
      lomiri = pkgs.lomiri.lomiri;
      settings = pkgs.lomiri.lomiri-system-settings;
    };
  };
}
