# Lomiri polish for the FP6 (2026-10-24): landscape OSK geometry, reachable
# Log Out / Restart / Power off, and an orientation sensor claim that survives
# iio-sensor-proxy restarts. Single importable module; every part has its own
# switch and the whole module is a no-op unless `enable` is set.
#
#  * oskLandscape: Lomiri publishes its (rotated) input-method area to
#    $XDG_RUNTIME_DIR/lomiri-input-panel-size; maliit-keyboard sizes its
#    input-panel window from it. zwp_input_panel_v1 has no configure event
#    and Lomiri rotates in QML, so Maliit always laid out for the portrait
#    output (372x828 logical) and lomiri-input-method-scale.patch then
#    stretched that surface ~2.2x wide / ~0.45x high in landscape.
#  * sessionEntries: Panel.qml hid ayatana-indicator-session on screens
#    <= 60 gu (the FP6 is 46.5 gu), even in the expanded menus; it is now shown
#    there (still hidden in the collapsed status row). The power-key dialog
#    also gets "Log Out" in user sessions. Suspend stays absent: logind
#    reports CanSuspend=no (fp6 sleep block), so neither the indicator nor
#    Lomiri offers it.
#  * sensorReclaim: patched QtSensors iio-sensor-proxy orientation backend
#    (own identifier, only on Lomiri's QT_PLUGIN_PATH) that stays active and
#    re-claims the accelerometer when net.hadess.SensorProxy gets a new owner.
#    Sensors.conf pins QOrientationSensor to it; processes without the plugin
#    fall back to the first registered backend (stock iio-sensor-proxy).
#  * cameraCutout: the FP6 front camera hole sits at the top centre of the
#    portrait panel, and Lomiri 0.5.0 has no display-cutout support. In
#    portrait the collapsed indicator row is limited to the part of the panel
#    right of a centred gap (cameraCutoutGap grid units); indicators that do
#    not fit are hidden from the collapsed row, as on any overflow, and are
#    still in the pulled-down menus. Landscape keeps the full width.
#  * sessionIndicatorLogout: ayatana-indicator-session already exports
#    "Log Out…" (indicator.logout) in the menu the phone profile uses, but
#    Lomiri's IndicatorMenuItemFactory drops indicator.logout (and suspend,
#    hibernate, reboot) unless Platform.isPC. Logout is no longer filtered.
#    Tapping it: indicator -> org.gnome.SessionManager EndSessionDialog.Open
#    (Lomiri) -> RequestLogout -> Lomiri's Lock / Log Out / Cancel dialog ->
#    logout() -> LogoutReady -> close all windows -> Qt.quit(). A 10 s
#    fallback timer quits anyway if some window never closes, so a stuck app
#    cannot leave the shell hanging after Log Out.
#  * launcherLockedDefaultOff: the AccountsService default of
#    com.lomiri.AccountsService.SecurityPrivacy EnableLauncherWhileLocked
#    (lomiri-schemas XML) becomes false, so the left-edge launcher is not
#    available on the lock screen unless the user turns it on in System
#    Settings > Security & Privacy > Locked phone. Shipped as a hiPrio copy
#    of the XML in the system profile (accounts-daemon reads
#    system-path/share/accountsservice/interfaces), so lomiri-schemas and
#    its many reverse dependencies are not rebuilt. Users who already wrote
#    the key keep their value. EnableIndicatorsWhileLocked stays true.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriPolish;
  canExecute = pkgs.stdenv.buildPlatform.canExecute pkgs.stdenv.hostPlatform;
  sensorsConf = pkgs.writeTextDir "etc/xdg/QtProject/Sensors.conf" ''
    [Default]
    QOrientationSensor=fp6-iio-sensor-proxy.orientationsensor
    QLightSensor=iio-sensor-proxy.lightsensor
  '';
  securityPrivacyDefaults = pkgs.runCommand "fp6-lomiri-securityprivacy-defaults" { } ''
    iface=com.lomiri.AccountsService.SecurityPrivacy.xml
    src=${pkgs.lomiri.lomiri-schemas}/share/dbus-1/interfaces/$iface
    mkdir -p $out/share/dbus-1/interfaces $out/share/accountsservice/interfaces
    # Rewrite only the EnableLauncherWhileLocked default (true -> false).
    sed '/<property name="EnableLauncherWhileLocked"/,/<\/property>/ s/value="true"/value="false"/' \
      $src > $out/share/dbus-1/interfaces/$iface
    grep -A1 'name="EnableLauncherWhileLocked"' $out/share/dbus-1/interfaces/$iface | grep -q 'value="false"'
    grep -A1 'name="EnableIndicatorsWhileLocked"' $out/share/dbus-1/interfaces/$iface | grep -q 'value="true"'
    [ "$(diff $src $out/share/dbus-1/interfaces/$iface | grep -c '^>')" = 1 ]
    ln -s ../../dbus-1/interfaces/$iface $out/share/accountsservice/interfaces/$iface
  '';
  overlay = final: prev: {
    fp6-qtsensors-iio-reclaim =
      final.libsForQt5.callPackage ./qtsensors-iio-reclaim/package.nix { };

    maliit-keyboard = if !cfg.oskLandscape then prev.maliit-keyboard else
      prev.maliit-keyboard.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./maliit-keyboard-panel-size-hint.patch ];
        postBuild = (old.postBuild or "") + lib.optionalString canExecute ''
          $CXX -std=c++17 -fPIC -I../src/plugin ../tests/fp6/panelsizehint_test.cpp \
            $(pkg-config --cflags --libs Qt5Core) -o fp6-panelsizehint-test
          ./fp6-panelsizehint-test
        '';
      });

    lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
      lomiri = lprev.lomiri.overrideAttrs (old: {
        patches = (old.patches or [ ])
          ++ lib.optional cfg.oskLandscape ./lomiri-input-panel-size-hint.patch
          ++ lib.optional cfg.sessionEntries ./lomiri-session-entries-phone.patch;
        postPatch = (old.postPatch or "") + lib.optionalString cfg.cameraCutout ''
          substituteInPlace qml/Panel/Panel.qml \
            --replace-fail '    property real expandedPanelHeight: units.gu(7)' \
              '    property real expandedPanelHeight: units.gu(7)
              // FP6: keep the collapsed indicator row right of the front camera hole (portrait).
              readonly property real cameraCutoutGap: units.gu(${toString cfg.cameraCutoutGap})
              readonly property real cameraCutoutRightWidth: width < height ? (width - cameraCutoutGap) / 2 : width' \
            --replace-fail '            overFlowWidth: width - appMenuClear' \
              '            overFlowWidth: Math.min(width - appMenuClear, root.cameraCutoutRightWidth)'
        '' + lib.optionalString cfg.sessionIndicatorLogout ''
          # FP6: keep "Log Out…" of the session indicator on phones too.
          substituteInPlace qml/Panel/Indicators/IndicatorMenuItemFactory.qml \
            --replace-fail '"indicator.logout": Platform.isPC ? undefined : null,' \
              '"indicator.logout": undefined, // FP6: Log Out also on phones'
          substituteInPlace qml/Components/Dialogs.qml \
            --replace-fail '    property var doOnClosedAllWindows: function() {}' \
              '    property var doOnClosedAllWindows: function() {}
              // FP6: if some window never closes after Log Out, end the session anyway.
              Timer {
                  id: logoutFallbackTimer
                  interval: 10000
                  onTriggered: {
                      console.warn("FP6: windows still open 10 s after Log Out, quitting anyway");
                      Qt.quit();
                      root.lomiriSessionService.endSession();
                  }
              }' \
            --replace-fail '            topLevelSurfaceList.closeAllWindows();
        }
    }
}' \
              '            topLevelSurfaceList.closeAllWindows();
            logoutFallbackTimer.start();
        }
    }
}'
        '';
        preFixup = (old.preFixup or "") + lib.optionalString cfg.sensorReclaim ''
          qtWrapperArgs+=(--prefix QT_PLUGIN_PATH : ${final.fp6-qtsensors-iio-reclaim}/${final.libsForQt5.qtbase.qtPluginPrefix})
        '';
      });
    });
  };
in {
  options.hardware.fairphone6.lomiriPolish = {
    enable = lib.mkEnableOption "the FP6 Lomiri polish set (OSK landscape, session entries, sensor re-claim)";
    oskLandscape = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Lay Maliit out for Lomiri's rotated input-method area (landscape OSK fix).";
    };
    sessionEntries = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Show the session indicator in the expanded panel and add Log Out to the power dialog.";
    };
    cameraCutout = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Keep the collapsed status-bar indicators right of the top-centre camera hole in portrait.";
    };
    cameraCutoutGap = lib.mkOption {
      type = lib.types.ints.positive;
      default = 4;
      description = "Width in grid units of the centred status-bar gap kept free for the camera hole.";
    };
    sessionIndicatorLogout = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Show the session indicator's Log Out item on phones (Lomiri hides it unless Platform.isPC), with a fallback quit if windows do not close.";
    };
    launcherLockedDefaultOff = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Default EnableLauncherWhileLocked (AccountsService SecurityPrivacy) to false: no launcher on the lock screen unless enabled in Settings.";
    };
    glyphCacheWorkaround = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Set QML_USE_GLYPHCACHE_WORKAROUND=1 (Qt5 Quick keeps a system-memory copy
        of the distance-field glyph cache and re-uploads it on resize instead of
        a GPU texture copy). Text turned into white boxes on freedreno after the
        glyph cache grew (2026-10-26).
      '';
    };
    sensorReclaim = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Use the QtSensors orientation backend that re-claims after iio-sensor-proxy restarts.";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      # After configuration.nix / lomiri-gpu-fp6.nix (both lib.mkAfter = 1500),
      # so these patches apply on top of the existing fp6 Lomiri patch set.
      nixpkgs.overlays = lib.mkOrder 2000 [ overlay ];
      system.build.fp6-lomiri-polish = {
        inherit (pkgs) maliit-keyboard fp6-qtsensors-iio-reclaim;
        lomiri = pkgs.lomiri.lomiri;
        inherit securityPrivacyDefaults;
      };
    }
    (lib.mkIf cfg.launcherLockedDefaultOff {
      # Wins over lomiri-schemas' copy of the same two paths in system-path.
      environment.systemPackages = [ (lib.hiPrio securityPrivacyDefaults) ];
    })
    (lib.mkIf cfg.glyphCacheWorkaround {
      # Login shells/apps and every systemd user unit (lomiri.service, the
      # greeter's, lomiri-app-launch apps).
      environment.sessionVariables.QML_USE_GLYPHCACHE_WORKAROUND = "1";
      systemd.user.settings.Manager.DefaultEnvironment = "QML_USE_GLYPHCACHE_WORKAROUND=1";
    })
    (lib.mkIf cfg.sensorReclaim {
      # Two copies are read: /etc/xdg (environment.etc) and the system profile
      # (sensors-lomiri.nix adds its file to systemPackages). QtSensors reads
      # the *last* XDG config dir, i.e. /run/current-system/sw/etc/xdg, so the
      # profile copy needs a higher priority than sensors-lomiri.nix's.
      environment.etc."xdg/QtProject/Sensors.conf".source =
        lib.mkForce "${sensorsConf}/etc/xdg/QtProject/Sensors.conf";
      environment.systemPackages = [ (lib.hiPrio sensorsConf) ];
    })
    {
      assertions = [{
        assertion = config.services.desktopManager.lomiri.enable;
        message = "lomiri-polish-fp6.nix only applies to the Lomiri session.";
      }];
    }
  ]);
}
