{ lib, rustPlatform, dbus, makeWrapper, qt5, lomiri }:
let
  qmlPackages = [ (lib.getBin qt5.qtdeclarative) qt5.qtgraphicaleffects qt5.qtquickcontrols lomiri.lomiri-ui-toolkit ];
in rustPlatform.buildRustPackage {
  pname = "fp6-callkit";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let name = baseNameOf path; in
      !(builtins.elem name [ "target" "patches" "previews" ".git" ".agents" ".codex" ".aws" "result"
        "README.md" "DESIGN.md" "FINAL_REPORT.md" "TASK.md" "docs" ])
      && !(lib.hasPrefix "result-" name);
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ makeWrapper ];
  nativeCheckInputs = [ dbus ];
  doCheck = true;
  postInstall = ''
    mkdir -p $out/share/fp6-callkit $out/share/applications
    cp -r qml $out/share/fp6-callkit/
    cp data/fp6-calls-preview.desktop $out/share/applications/
    makeWrapper ${lib.getDev qt5.qtdeclarative}/bin/qmlscene $out/bin/fp6-calls-preview \
      --add-flags "$out/share/fp6-callkit/qml/Preview.qml" \
      --prefix QML2_IMPORT_PATH : "${lib.makeSearchPath qt5.qtbase.qtQmlPrefix qmlPackages}" \
      --prefix QT_PLUGIN_PATH : "${lib.makeSearchPath qt5.qtbase.qtPluginPrefix [ qt5.qtbase (lib.getBin qt5.qtdeclarative) ]}"
  '';
  meta = {
    description = "Experimental Fairphone 6 session call broker and Lomiri UI prototype";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-callsd";
  };
}
