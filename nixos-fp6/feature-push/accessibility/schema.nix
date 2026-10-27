# The com.fairphone6.accessibility schema alone (Lomiri's wrapper, the
# Settings plugin and the audio bridge read it through XDG_DATA_DIRS).
{ stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "fp6-accessibility-schema";
  version = "0.1.0";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.accessibility.gschema.xml} $schemas/com.fairphone6.accessibility.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
