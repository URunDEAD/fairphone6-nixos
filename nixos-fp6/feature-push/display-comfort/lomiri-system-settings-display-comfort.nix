# System Settings > Display Comfort: pure-QML plugin (manifest + pages) plus
# the com.fairphone6.display-comfort schema for the wrapped settings app.
{ lib, stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-display-comfort";
  version = "0.1.0";
  src = ./settings-page;
  nativeBuildInputs = [ glib ];
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 display-comfort.settings $out/share/lomiri-system-settings/display-comfort.settings
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/display-comfort *.qml
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.display-comfort.gschema.xml} $schemas/com.fairphone6.display-comfort.gschema.xml
    glib-compile-schemas --strict $schemas
    runHook postInstall
  '';
  meta = {
    description = "Display Comfort page (night light, greyscale, dark theme, text and display size) for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
