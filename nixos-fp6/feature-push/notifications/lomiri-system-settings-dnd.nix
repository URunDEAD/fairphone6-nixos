# System Settings > Do Not Disturb: pure-QML plugin (manifest + pages) plus
# the com.fairphone6.notifications schema, so the wrapped
# lomiri-system-settings (wrapGAppsHook over its plugins) finds it.
{ lib, stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-do-not-disturb";
  version = "0.1.0";
  src = ./settings-page;
  nativeBuildInputs = [ glib ];
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 do-not-disturb.settings $out/share/lomiri-system-settings/do-not-disturb.settings
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/do-not-disturb *.qml
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.notifications.gschema.xml} $schemas/com.fairphone6.notifications.gschema.xml
    glib-compile-schemas --strict $schemas
    runHook postInstall
  '';
  meta = {
    description = "Do Not Disturb page (schedules, exceptions, bedtime, notification history) for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
