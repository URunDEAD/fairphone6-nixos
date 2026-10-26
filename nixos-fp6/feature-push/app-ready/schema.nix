# The com.fairphone6.app-ready schema alone, for wrappers that only need to
# read it (Lomiri's Stage.qml, System Settings).
{ stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "fp6-app-ready-schema";
  version = "0.1.0";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.app-ready.gschema.xml} $schemas/com.fairphone6.app-ready.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
