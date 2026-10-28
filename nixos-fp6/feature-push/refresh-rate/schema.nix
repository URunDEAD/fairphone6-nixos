# The com.fairphone6.refresh-rate schema (plus extra schemas, e.g. the
# battery one for battery saver), for Lomiri's wrapper (wrapGAppsHook adds
# its XDG_DATA_DIRS entry) and the system profile.
{ lib, stdenvNoCC, glib, extraSchemas ? [ ] }:
stdenvNoCC.mkDerivation {
  pname = "fp6-refresh-rate-schema";
  version = "0.1.0";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.refresh-rate.gschema.xml} $schemas/com.fairphone6.refresh-rate.gschema.xml
    ${lib.concatMapStrings (s: ''
      install -Dm644 ${s} $schemas/${baseNameOf (toString s)}
    '') extraSchemas}
    glib-compile-schemas --strict $schemas
  '';
}
