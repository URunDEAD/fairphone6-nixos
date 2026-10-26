# System Settings > App Launch: pure-QML plugin (manifest + pages) plus the
# com.fairphone6.app-ready schema, so the wrapped lomiri-system-settings
# (glib/wrapGAppsHook over its plugins) finds it.
{ lib, stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-app-launch";
  version = "0.1.0";
  src = ./settings-page;
  nativeBuildInputs = [ glib ];
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 app-launch.settings $out/share/lomiri-system-settings/app-launch.settings
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/app-launch *.qml
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.app-ready.gschema.xml} $schemas/com.fairphone6.app-ready.gschema.xml
    glib-compile-schemas --strict $schemas
    runHook postInstall
  '';
  meta = {
    description = "App Launch page (keep apps ready, prelaunch, preload, zram) for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
