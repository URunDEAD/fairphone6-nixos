# Charging feedback (feature push 2026-10-24): "when plugging the phone in
# it should show something", also on the locked screen. Android/iOS style.
# Notes, previews and the phone checklist:
# results/feature-push-20261024/charging-feedback/README.md
#
#  * fp6-charging-feedback (Rust, user service): kobject uevents
#    (power_supply, typec) -> debounced plug/unplug (0.3 s / 0.8 s, so a
#    power-role swap or a contact bounce is not an event), reads the
#    qcom_battmgr supplies + UCSI PD contract (read-only), fp6-batteryd's
#    status.json (charge limit, adaptive hold, time to full to the limit)
#    and fp6-usbmoded's status.json (a computer is connected; reverse
#    charging is not "plugged in"). Charging speed "fast"/"normal"/"slow"
#    from the measured power once settled (PD >= 15 W counts as fast at
#    once). Publishes org.gtk.Actions com.fairphone6.ChargingFeedback
#    (state JSON; wake = powerd Peek; refresh; simulate).
#  * Lomiri (lomiri-charging-feedback.patch for Greeter/, an anchored
#    insertion in Shell.qml, qml/; overlay lib.mkOrder 3900, in postPatch
#    after every other module's Lomiri edits):
#    Components/Fp6ChargingFeedback.qml over everything in Shell.qml:
#    plug-in animation (full screen over the lock screen or after waking
#    the display, a banner under the status bar while unlocked, none while
#    a computer's "Charging via USB" notification is the pop-up), sound
#    (freedesktop power-plug/unplug; not in silent mode / DND / a call) and
#    a haptic tick (not in DND). Greeter/Fp6ChargingStatus.qml: the line
#    under the lock-screen clock while plugged in ("Charging · 45 % · Full
#    in 1 h 10 min", "Charging paused at 80 % to protect the battery"),
#    replacing Lomiri's own "... until full" hint at the bottom.
#  * System Settings > Battery > Charging feedback (row added to the
#    battery module's Fp6BatteryExtras.qml): Charging animation, Turn the
#    screen on, Charging sound, Vibrate on plug, Show charging info on lock
#    screen (all default on), Preview. GSettings com.fairphone6.charging-feedback.
#  * Status bar: unchanged; ayatana-indicator-power already shows the
#    suru battery-NNN-charging (bolt) icons while charging (README).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.chargingFeedback;
  pkg = pkgs.callPackage ./package.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
  sounds = "${pkgs.sound-theme-freedesktop}/share/sounds/freedesktop/stereo";
  # Shell.qml: the instance goes right before the stock cursor-hiding timer
  # (an anchor, like lock-sounds 3800, so both insertions compose).
  shellAnchor = "    Timer {\n        id: cursorHidingTimer\n";
  shellInsert = lib.concatStringsSep "\n" [
    "    // FP6 charging feedback (nixos-fp6/feature-push/charging-feedback):"
    "    // plug-in animation (full screen over the lock screen, a banner while"
    "    // unlocked), sound and haptic tick; above the shell and the greeter,"
    "    // below edge lighting. Never takes input."
    "    Fp6ChargingFeedback {"
    "        id: fp6ChargingFeedback"
    "        anchors.fill: parent"
    "        z: cursor.z + 1.5"
    "        locked: shell.showingGreeter"
    "        inCall: callManager.hasCalls"
    "        panelHeight: panel.panelHeight"
    "    }"
    "" ""
  ];
  qmlFiles = {
    "Components/Fp6ChargingModel.qml" = ./qml/Fp6ChargingModel.qml;
    "Components/Fp6ChargingRing.qml" = ./qml/Fp6ChargingRing.qml;
    "Components/Fp6ChargingBolt.qml" = ./qml/Fp6ChargingBolt.qml;
    "Components/Fp6ChargingFeedback.qml" = ./qml/Fp6ChargingFeedback.qml;
    "Greeter/Fp6ChargingStatus.qml" = ./qml/Fp6ChargingStatus.qml;
  };
in {
  options.hardware.fairphone6.chargingFeedback.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Charging feedback: plug-in animation (lock screen, display off, banner
      while unlocked), sound and vibration, the lock-screen charging line,
      and Settings > Battery > Charging feedback (everything on by default).
    '';
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = (config.services.fp6LomiriSensors.enable or false) && config.services.fp6LomiriSensors.screenPower;
        message = "chargingFeedback wakes the display through fp6-lomiri-powerd's Peek (services.fp6LomiriSensors.enable + screenPower)."; }
      { assertion = config.hardware.fairphone6.battery.enable or false;
        message = "chargingFeedback adds its Settings row to the battery module's page and reads fp6-batteryd's status (hardware.fairphone6.battery.enable)."; }
      { assertion = config.hardware.fairphone6.lockscreenPolish.enable or false;
        message = "chargingFeedback's GreeterView/CoverPage patch context includes lockscreen-polish and the fingerprint hooks (hardware.fairphone6.lockscreenPolish.enable)."; }
    ];

    # After every other Lomiri overlay (highest so far: lock/unlock sounds 3800).
    nixpkgs.overlays = lib.mkOrder 3900 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          # The QML reads com.fairphone6.charging-feedback (wrapGAppsHook).
          buildInputs = (old.buildInputs or [ ]) ++ [ pkg.schema ];
          postPatch = (old.postPatch or "") + ''
            # FP6 charging feedback (after every other module's Lomiri edits)
            patch -p1 --no-backup-if-mismatch < ${./lomiri-charging-feedback.patch}
            substituteInPlace qml/Shell.qml \
              --replace-fail ${lib.escapeShellArg shellAnchor} ${lib.escapeShellArg (shellInsert + shellAnchor)}
            ${lib.concatStrings (lib.mapAttrsToList (dst: src: ''
              install -m644 ${src} qml/${dst}
            '') qmlFiles)}
            substituteInPlace qml/Components/Fp6ChargingFeedback.qml \
              --replace-fail '@fp6ChargingPlugSound@' '${sounds}/power-plug.oga' \
              --replace-fail '@fp6ChargingUnplugSound@' '${sounds}/power-unplug.oga'
            grep -q 'Fp6ChargingFeedback {' qml/Shell.qml
            grep -q 'Fp6ChargingStatus {' qml/Greeter/GreeterView.qml
            grep -q 'fp6HideChargingHint' qml/Greeter/CoverPage.qml
            test -e ${sounds}/power-plug.oga && test -e ${sounds}/power-unplug.oga
          '';
          passthru = (old.passthru or { }) // { fp6ChargingFeedback = true; };
        });
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        })).overrideAttrs (old: {
          # Battery page: a "Charging feedback" row in the FP6 extras
          # (patched copy in the wrapper's merged tree, as battery-fp6.nix
          # does for the stock page).
          postInstall = (old.postInstall or "") + ''
            extras=$out/share/lomiri-system-settings/qml-plugins/battery/Fp6BatteryExtras.qml
            test -e "$extras"
            cp --remove-destination "$(readlink -f "$extras")" "$extras"
            chmod u+w "$extras"
            patch "$extras" < ${./settings-page/lss-battery-extras-charging-feedback.patch}
            grep -q Fp6ChargingFeedbackPage.qml "$extras"
            test -e $out/share/lomiri-system-settings/qml-plugins/battery/Fp6ChargingFeedbackPage.qml
            grep -q Fp6BatteryExtras.qml $out/share/lomiri-system-settings/qml-plugins/battery/PageComponent.qml
          '';
        });
      });
    }) ];

    environment.systemPackages = [ pkg.core pkg.schema ];

    systemd.user.services.fp6-charging-feedback = {
      description = "FP6 charging feedback (charger plug detection and charging state for Lomiri)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = lib.getExe pkg.core;
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 5;
      };
    };

    system.build.fp6-charging-feedback = {
      inherit (pkg) core schema lssPlugin;
      lomiri = pkgs.lomiri.lomiri;
      lomiri-system-settings = pkgs.lomiri.lomiri-system-settings;
    };
  };
}
