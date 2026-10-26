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
#    portrait the collapsed indicator row keeps a centred gap (cameraCutoutGap
#    grid units) free: indicators fill the part right of it first, and the
#    ones that do not fit there continue left of the gap (shifted past it)
#    instead of all being dropped. Only what would reach the app-menu or
#    recording-dot area at the left, or any left item while the call hint
#    shows, is hidden; everything stays in the pulled-down menus. Landscape
#    keeps the full width.
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
              readonly property real cameraCutoutRightWidth: width < height ? (width - cameraCutoutGap) / 2 : width
              // FP6: indicators that do not fit right of the hole continue left of it.
              // cameraCutoutUsed = width taken right of the hole by the items that fit there.
              // cameraCutoutExtent = distance from the right edge to the leftmost drawn item
              // (the indicators drag/tap area is widened to it).
              property real cameraCutoutUsed: 0
              property real cameraCutoutExtent: 0
              property var cameraCutoutItems: []
              function cameraCutoutUpdate() {
                  var used = 0, extent = 0;
                  for (var i = 0; i < cameraCutoutItems.length; i++) {
                      var it = cameraCutoutItems[i];
                      if (it.fp6FitsRight && it.width > 0) used = Math.max(used, it.fp6DistRight);
                      if (!it.hidden && it.width > 0) extent = Math.max(extent, it.fp6DistRight + it.fp6Shift);
                  }
                  cameraCutoutUsed = used;
                  cameraCutoutExtent = extent;
              }' \
            --replace-fail '            overFlowWidth: width - appMenuClear' \
              '            overFlowWidth: Math.min(width - appMenuClear, root.cameraCutoutRightWidth)
                      dragOverFlowWidth: Math.min(width - appMenuClear, Math.max(overFlowWidth, root.cameraCutoutExtent))' \
            --replace-fail '                readonly property bool overflow: parent.width - (x - __indicators.rowContentX) > __indicators.overFlowWidth' \
              '                // FP6: an item that does not fit right of the camera hole is drawn
                // left of it (shifted past the gap); it is hidden only when it would
                // reach the app-menu/recording area at the left, or during a call hint.
                readonly property real fp6DistRight: parent.width - (x - __indicators.rowContentX)
                readonly property bool fp6FitsRight: fp6DistRight <= __indicators.overFlowWidth
                readonly property bool fp6LeftOfCutout: !expanded && !fp6FitsRight
                                                        && root.cameraCutoutRightWidth < __indicators.width
                readonly property real fp6Shift: fp6LeftOfCutout
                    ? root.cameraCutoutGap + __indicators.overFlowWidth - root.cameraCutoutUsed : 0
                readonly property real fp6LeftLimit: __indicators.width - __indicators.appMenuClear
                    - units.gu(1) - (root.screenRecording ? units.gu(2.5) : 0)
                readonly property bool overflow: fp6LeftOfCutout
                    ? (callHint.visible || fp6DistRight + fp6Shift > fp6LeftLimit)
                    : !fp6FitsRight
                onFp6DistRightChanged: Qt.callLater(root.cameraCutoutUpdate)
                onFp6FitsRightChanged: Qt.callLater(root.cameraCutoutUpdate)
                onWidthChanged: Qt.callLater(root.cameraCutoutUpdate)
                onFp6ShiftChanged: Qt.callLater(root.cameraCutoutUpdate)
                onHiddenChanged: Qt.callLater(root.cameraCutoutUpdate)
                Component.onCompleted: {
                    root.cameraCutoutItems.push(indicatorItem);
                    Qt.callLater(root.cameraCutoutUpdate);
                }
                Component.onDestruction: {
                    var i = root.cameraCutoutItems.indexOf(indicatorItem);
                    if (i >= 0) root.cameraCutoutItems.splice(i, 1);
                    Qt.callLater(root.cameraCutoutUpdate);
                }
                transform: Translate { x: -indicatorItem.fp6Shift }'
          # The drag/tap area covers the items drawn left of the hole too, and a
          # press there selects the item drawn under it, not its unshifted slot.
          substituteInPlace qml/Panel/PanelMenu.qml \
            --replace-fail '    property alias overFlowWidth: bar.overFlowWidth' \
              '    property alias overFlowWidth: bar.overFlowWidth
              property real dragOverFlowWidth: overFlowWidth // FP6: camera cutout' \
            --replace-fail '        width: root.overFlowWidth + root.menuContentX' \
              '        width: root.dragOverFlowWidth + root.menuContentX' \
            --replace-fail '                touchPressTime = new Date().getTime();' \
              '                touchPressTime = new Date().getTime();
                          // FP6: collapsed bar, items left of the camera hole are shifted
                          bar.fp6PressAt(mapToItem(bar, touchPosition.x, 0).x);'
          substituteInPlace qml/Panel/PanelItemRow.qml \
            --replace-fail '        var item = row.itemAt(x + row.contentX, y);
                  return item && item.hasOwnProperty("ownIndex") ? item : null;' \
              '        // FP6: items left of the camera hole are drawn shifted (fp6Shift);
                  // hit-test them where they are drawn.
                  var cx = x + row.contentX;
                  for (var i = 0; i < row.contentItem.children.length; i++) {
                      var c = row.contentItem.children[i];
                      if (c.hasOwnProperty("ownIndex") && c.hasOwnProperty("fp6Shift") && c.fp6Shift > 0 &&
                              c.width > 0 && cx >= c.x - c.fp6Shift && cx < c.x - c.fp6Shift + c.width)
                          return c;
                  }
                  var item = row.itemAt(cx, y);
                  if (item && item.hasOwnProperty("fp6Shift") && item.fp6Shift > 0) return null;
                  return item && item.hasOwnProperty("ownIndex") ? item : null;' \
            --replace-fail '    function selectItemAt(lateralPosition) {
                  var item = indicatorAt(lateralPosition, 0);' \
              '    // FP6: the shifted item drawn under the finger when the collapsed bar was
              // pressed; the next selection uses it (the bar is expanded by then and the
              // shift is gone), and fp6PressShift realigns the expanded row to the finger.
              property var fp6PressItem: null
              property real fp6PressShift: 0
              function fp6PressAt(x) {
                  var item = indicatorAt(x, 0);
                  fp6PressItem = (item && item.fp6Shift > 0 && item.opacity > 0 && item.enabled) ? item : null;
                  fp6PressShift = fp6PressItem ? fp6PressItem.fp6Shift : 0;
              }
          
              function selectItemAt(lateralPosition) {
                  if (fp6PressItem) {
                      row.currentIndex = fp6PressItem.ownIndex;
                      fp6PressItem = null;
                      return;
                  }
                  fp6PressShift = 0;
                  var item = indicatorAt(lateralPosition, 0);'
          substituteInPlace qml/Panel/PanelBar.qml \
            --replace-fail '    function selectPreviousItem() {' \
              '    function fp6PressAt(lateralPosition) { // FP6: camera cutout
                  row.fp6PressAt(root.mapToItem(row, lateralPosition, 0).x);
              }
          
              function selectPreviousItem() {' \
            --replace-fail '                originalDistanceFromEdge = initialItem ? (row.width - initialItem.x - initialItem.width/2) : -1;' \
              '                originalDistanceFromEdge = initialItem ? (row.width - initialItem.x - initialItem.width/2 + row.fp6PressShift) : -1;'
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
