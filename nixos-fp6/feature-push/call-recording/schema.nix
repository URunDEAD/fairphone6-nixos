# com.fairphone6.callrecording GSettings schema alone (the dialer and
# fp6-callrec use it; a schema-only package keeps the dialer from rebuilding
# when the Rust code changes).
{ stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  name = "fp6-callrecording-schema-1";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.callrecording.gschema.xml} $schemas/com.fairphone6.callrecording.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
