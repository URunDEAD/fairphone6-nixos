# System Settings > Gesture Shortcuts: pure-QML plugin (manifest + page) plus
# the com.fairphone6.gestures schema, so the wrapped lomiri-system-settings
# (glib/wrapGAppsHook over its plugins) finds it.
{ lib, stdenvNoCC, glib, fp6-gestures }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-gesture-shortcuts";
  version = fp6-gestures.version;
  src = ./settings-page;
  nativeBuildInputs = [ glib ];
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 gesture-shortcuts.settings $out/share/lomiri-system-settings/gesture-shortcuts.settings
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/gesture-shortcuts PageComponent.qml ActionSettings.qml BackTapSettings.qml
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${fp6-gestures.schemaXml} $schemas/com.fairphone6.gestures.gschema.xml
    glib-compile-schemas --strict $schemas
    runHook postInstall
  '';
  meta = {
    description = "Gesture Shortcuts page (shake, power key double/triple press) for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
