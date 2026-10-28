# Refresh rate ("Smooth display") for Lomiri on the FP6 (feature push
# 2026-10-24). Findings, design, battery cost and the supervised test plan:
# results/feature-push-20261024/refresh-rate/README.md.
#
# DISABLED BY DEFAULT (hardware.fairphone6.refreshRate.enable = false) until
# a supervised RAM-boot test on the phone.
#
#  * Panel: BOE BJ631JHM-T71-D900, Novatek NT37705, command mode, DSC.
#    kernel6 exposes ONE mode, 1116x2484 @ 120 Hz, which is what runs today.
#  * Kernel candidates (kernel-build/feature-push/kernel7-candidates, not in
#    kernel6, not booted): 0009 adds 90/60/30 Hz DRM modes (vendor timings;
#    a change is a full modeset, the screen blanks briefly); 0010 adds
#    panel_rate_hz (120/60/30/10 Hz on the 120 Hz scan, no modeset) on the
#    DSI device, which the adaptive mode uses.
#  * Shell (Lomiri, Fp6RefreshRate.qml + FP6.RefreshRate C++ plugin):
#    fixed modes through WindowManager.Screen.applyConfiguration() (qtmir
#    ScreensController -> Mir set_base_configuration -> KMS); adaptive =
#    120 Hz mode, panel at 120 Hz while touched (ActivityWatcher event
#    filter on the shell window) and at adaptive-idle-rate when idle.
#    Battery saver (com.fairphone6.battery saver-active) caps at 60 Hz.
#    No Mir or qtmir patch: Lomiri's WindowManager plugin already exposes
#    availableModes/currentModeIndex/beginConfiguration/applyConfiguration.
#    (Mir's persisted display config is not used: miroil skips the internal
#    panel because it has no EDID - "Failed to parse EDID" in the journal.)
#  * Settings: System Settings > Brightness & Display > Smooth display
#    (only the modes the running kernel offers; a never-kept mode runs on a
#    15 s trial and goes back unless kept). Quick-settings tile
#    "smooth-display" (opt-in, from the editor's tray).
#  * With kernel6 the shell sees one rate and the setting reports "only
#    120 Hz": nothing changes on the phone.
#
# Overlay order lib.mkOrder 3850: after quick-settings (3100, copies
# QsBackend.qml), battery (2900, Battery page), accessibility (3600,
# QsBackend catalogue), inline replies (3700) and lock sounds (3800; no
# shared files); before charging feedback (3900).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.refreshRate;
  batteryOn = config.hardware.fairphone6.battery.enable or false;
  quickSettingsOn = config.hardware.fairphone6.quickSettings.enable or false;
  user = config.services.fp6LomiriSensors.user or "user";
  schema = pkgs.callPackage ./schema.nix {
    extraSchemas = lib.optional batteryOn ../battery/com.fairphone6.battery.gschema.xml;
  };
  qmlPlugin = pkgs.callPackage ./qml-plugin/package.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-refresh-rate.nix { };
  tileAnchor = "    // ---- page tiles: the indicator pages that have no switch tile of their own";
in {
  options.hardware.fairphone6.refreshRate = {
    enable = lib.mkEnableOption ''
      the refresh rate setting (System Settings > Brightness & Display >
      Smooth display, quick-settings tile) for Lomiri. Needs the kernel7
      candidate panel patches for more than 120 Hz; off until a supervised
      phone test'';
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ schema ];
    system.build.fp6-refresh-rate = { inherit schema qmlPlugin lssPlugin; lomiri = pkgs.lomiri.lomiri; };

    # panel_rate_hz (kernel candidate 0010) writable by the session user's
    # video group, so the shell can set the panel's runtime rate. The kernel
    # validates every value; without 0010 the attribute does not exist.
    services.udev.extraRules = ''
      ACTION!="remove", SUBSYSTEM=="mipi-dsi", DRIVER=="panel-novatek-nt37705", TEST=="panel_rate_hz", RUN+="${pkgs.coreutils}/bin/chgrp video /sys%p/panel_rate_hz", RUN+="${pkgs.coreutils}/bin/chmod 0664 /sys%p/panel_rate_hz"
    '';
    assertions = [{
      assertion = lib.elem "video" (config.users.users.${user}.extraGroups or [ ]);
      message = "refreshRate: the session user must be in the video group (panel_rate_hz).";
    }];

    nixpkgs.overlays = lib.mkOrder 3850 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          # FP6.RefreshRate QML module (wrapQtAppsHook adds its QML dir) and
          # the schemas (wrapGAppsHook adds the XDG_DATA_DIRS entry).
          buildInputs = (old.buildInputs or [ ]) ++ [ qmlPlugin schema ];
          postPatch = (old.postPatch or "") + ''
            # FP6 refresh rate (qml/*.qml and *.js are installed by glob)
            cp ${./shell/Fp6RefreshRate.qml} qml/Fp6RefreshRate.qml
            cp ${./shell/Fp6RefreshRate.js} qml/Fp6RefreshRate.js
            substituteInPlace qml/ShellScreen.qml --replace-fail \
              '    DeviceConfiguration {' \
              '    // fp6 refresh rate (Smooth display): Fp6RefreshRate.qml
                Fp6RefreshRate {
                    screen: screenWindow.screen
                    window: screenWindow
                    active: screenWindow.primary
                }

                DeviceConfiguration {'
            grep -q 'Fp6RefreshRate {' qml/ShellScreen.qml
          '' + lib.optionalString quickSettingsOn ''
            qs=qml/Panel/QuickSettings/QsBackend.qml
            test -e $qs
            substituteInPlace $qs --replace-fail '${tileAnchor}' \
              "$(cat ${./quick-settings/SmoothDisplayTile.qml.in})
            ${tileAnchor}"
            # add the tile to the catalogue (end of its list)
            sed -i '/readonly property var catalogue: \[/,/\]/ s/\]$/, fp6RefreshRateTile]/' $qs
            grep -q 'fp6RefreshRateTile\]' $qs
            grep -q 'tileId: "smooth-display"' $qs
          '';
          passthru = (old.passthru or { }) // { fp6RefreshRate = true; };
        });
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
        })).overrideAttrs (old: {
          postInstall = (old.postInstall or "") + ''
            page=$out/share/lomiri-system-settings/qml-plugins/brightness/PageComponent.qml
            test -e "$page"
            cp --remove-destination "$(readlink -f "$page")" "$page"
            chmod u+w "$page"
            patch "$page" < ${./lss-brightness-smooth-display.patch}
            grep -q Fp6SmoothDisplay.qml "$page"
            test -e $out/share/lomiri-system-settings/qml-plugins/brightness/Fp6SmoothDisplay.qml
          '';
        });
      });
    }) ];
  };
}
