# System Settings > Brightness & Display > Smooth display: the QML block
# (loaded by the stock brightness page through lss-brightness-smooth-
# display.patch, applied in the wrapper), its policy helpers and the schema.
{ lib, stdenvNoCC, glib }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-refresh-rate";
  version = "0.1.0";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  installPhase = ''
    runHook preInstall
    dir=$out/share/lomiri-system-settings/qml-plugins/brightness
    install -Dm644 ${./settings-page/Fp6SmoothDisplay.qml} $dir/Fp6SmoothDisplay.qml
    install -Dm644 ${./shell/Fp6RefreshRate.js} $dir/Fp6RefreshRate.js
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.refresh-rate.gschema.xml} $schemas/com.fairphone6.refresh-rate.gschema.xml
    glib-compile-schemas --strict $schemas
    runHook postInstall
  '';
  meta = {
    description = "Smooth display (refresh rate) block for the Lomiri System Settings brightness page";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
