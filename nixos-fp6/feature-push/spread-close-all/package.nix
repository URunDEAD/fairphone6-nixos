# com.fairphone6.app-switcher schema, as a plain schema package (Lomiri's
# buildInputs: wrapGAppsHook adds it to XDG_DATA_DIRS) and as a System
# Settings plugin without a page (the wrapper picks up its schema for the
# App switcher section of Gesture Shortcuts).
{ lib, stdenvNoCC, glib }:
let
  mk = pname: extra: stdenvNoCC.mkDerivation {
    inherit pname;
    version = "0.1.0";
    src = ./com.fairphone6.app-switcher.gschema.xml;
    dontUnpack = true;
    nativeBuildInputs = [ glib ];
    installPhase = ''
      runHook preInstall
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 $src $schemas/com.fairphone6.app-switcher.gschema.xml
      glib-compile-schemas --strict $schemas
      ${extra}
      runHook postInstall
    '';
    meta = { description = "FP6 app switcher close-all settings schema"; license = lib.licenses.gpl3Only; platforms = lib.platforms.linux; };
  };
in {
  schema = mk "fp6-app-switcher-schema" "";
  # The wrapper lndir's these from every plugin.
  lssPlugin = mk "lomiri-system-settings-app-switcher-schema" ''
    mkdir -p $out/lib/lomiri-system-settings $out/share/lomiri-system-settings $out/share/locale
  '';
}
