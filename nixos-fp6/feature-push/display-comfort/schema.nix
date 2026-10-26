# The com.fairphone6.display-comfort schema alone, for Lomiri's wrapper
# (ShellScreen.qml reads it through wrapGAppsHook's XDG_DATA_DIRS).
{ stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "fp6-display-comfort-schema";
  version = "0.1.0";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.display-comfort.gschema.xml} $schemas/com.fairphone6.display-comfort.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
