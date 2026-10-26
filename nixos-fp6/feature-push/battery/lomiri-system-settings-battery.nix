# FP6 additions to System Settings > Battery: QML files installed into the
# stock battery plugin's directory (the wrapper lndir's every plugin into
# one tree) plus the com.fairphone6.battery schema for the wrapped app.
# The stock PageComponent.qml loads Fp6BatteryExtras.qml (patched in the
# wrapper by battery-fp6.nix).
{ lib, stdenvNoCC, glib, fp6-batteryd }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-battery-fp6";
  version = fp6-batteryd.version;
  src = ./settings-page;
  nativeBuildInputs = [ glib ];
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/battery Fp6*.qml
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${fp6-batteryd.schemaXml} $schemas/com.fairphone6.battery.gschema.xml
    glib-compile-schemas --strict $schemas
    runHook postInstall
  '';
  meta = {
    description = "Charge limit, adaptive charging, battery saver, health and usage pages for Lomiri System Settings > Battery";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
