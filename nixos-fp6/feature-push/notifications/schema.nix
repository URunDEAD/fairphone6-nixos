# The com.fairphone6.notifications schema alone, for Lomiri's wrapper
# (Notifications.qml, Wallpaper.qml) and System Settings.
{ stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "fp6-notifications-schema";
  version = "0.1.0";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.notifications.gschema.xml} $schemas/com.fairphone6.notifications.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
