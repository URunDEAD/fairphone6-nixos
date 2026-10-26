# com.fairphone6.privacy GSettings schema alone (Lomiri, the Settings plugin
# and the daemons use it; a schema-only package keeps Lomiri from rebuilding
# when the Rust code changes).
{ stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  name = "fp6-privacy-schema-1";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.privacy.gschema.xml} $schemas/com.fairphone6.privacy.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
