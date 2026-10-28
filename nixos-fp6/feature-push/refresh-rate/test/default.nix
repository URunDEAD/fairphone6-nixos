# Laptop (x86_64) checks of the refresh-rate feature; no phone, no ARM
# builder. Builds the FP6.RefreshRate plugin with its C++ test, the schemas
# (strict), the Settings block, and runs the QML tests offscreen:
# tst_policy.qml (Fp6RefreshRate.js) and tst_shell.qml (Fp6RefreshRate.qml
# with the real plugin, a mock Screen/GSettings and a fake panel_rate_hz).
#   nix-build nixos-fp6/feature-push/refresh-rate/test
{ nixpkgs ? (import ../../../upstream/mobile-nixos/npins).nixpkgs }:
let
  pkgs = import nixpkgs { system = "x86_64-linux"; };
  qt = pkgs.libsForQt5;
  plugin = pkgs.callPackage ../qml-plugin/package.nix { };
  schema = pkgs.callPackage ../schema.nix { extraSchemas = [ ../../battery/com.fairphone6.battery.gschema.xml ]; };
  lssPlugin = pkgs.callPackage ../lomiri-system-settings-refresh-rate.nix { };
  mockSchema = pkgs.stdenv.mkDerivation {
    name = "fp6-test-gsettings-mock";
    src = ./mock-schema;
    nativeBuildInputs = [ pkgs.cmake ];
    buildInputs = [ qt.qtbase qt.qtdeclarative ];
    dontWrapQtApps = true;
  };
in
pkgs.runCommand "fp6-refresh-rate-laptop-test" {
  nativeBuildInputs = [ qt.qtdeclarative.dev pkgs.glib ];
  passthru = { inherit plugin schema lssPlugin; };
} ''
  set -eu
  export HOME=$TMPDIR QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software
  export QT_PLUGIN_PATH=${pkgs.lib.getBin qt.qtbase}/${qt.qtbase.qtPluginPrefix}
  qml=${qt.qtbase.qtQmlPrefix}
  export QML2_IMPORT_PATH=${./mock}:${mockSchema}/qml:${plugin}/$qml:${qt.qtdeclarative}/$qml
  work=$TMPDIR/work; mkdir -p $work/sys
  # (store paths carry a hash prefix: copy each to its own name)
  cp ${../shell/Fp6RefreshRate.js} $work/Fp6RefreshRate.js
  cp ${../shell/Fp6RefreshRate.qml} $work/Fp6RefreshRate.qml
  cp ${./MockScreen.qml} $work/MockScreen.qml
  cp ${./tst_policy.qml} $work/tst_policy.qml
  echo 120 > $work/sys/panel_rate_hz
  echo '120 60 30 10' > $work/sys/panel_rates
  sed "s#@PANEL@#$work/sys/panel_rate_hz#" ${./tst_shell.qml.in} > $work/tst_shell.qml
  # schemas compile (strict) and contain the keys the QML uses
  test -s ${schema}/share/gsettings-schemas/*/glib-2.0/schemas/gschemas.compiled
  gsettings --schemadir ${schema}/share/gsettings-schemas/fp6-refresh-rate-schema-0.1.0/glib-2.0/schemas \
    list-keys com.fairphone6.refresh-rate | sort > keys.txt
  for k in mode adaptive-idle-rate adaptive-idle-ms saver-cap smooth-mode confirmed confirmed-modes available-rates adaptive-available active-rate; do
    grep -qx "$k" keys.txt
  done
  test -e ${lssPlugin}/share/lomiri-system-settings/qml-plugins/brightness/Fp6SmoothDisplay.qml
  qmltestrunner -platform offscreen -input $work/tst_policy.qml 2>&1 | tee policy.log
  qmltestrunner -platform offscreen -input $work/tst_shell.qml 2>&1 | tee shell.log
  grep -q ', 0 failed' policy.log
  grep -q ', 0 failed' shell.log
  # syntax of the QML that only runs on the phone
  qmllint ${../settings-page/Fp6SmoothDisplay.qml} ${../shell/Fp6RefreshRate.qml}
  mkdir -p $out
  cp policy.log shell.log keys.txt $out/
''
