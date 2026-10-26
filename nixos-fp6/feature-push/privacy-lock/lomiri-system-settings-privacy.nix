# Security & Privacy sub-pages (QML only) for the wrapped
# lomiri-system-settings; reached from the patched security-privacy pages
# (lss-security-privacy.patch, applied in the wrapper). No .settings file:
# no extra top-level entry.
{ lib, stdenvNoCC, fp6-privacy-schema }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-fp6-privacy";
  version = "1";
  src = ./settings-page;
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/fp6-privacy *.qml
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    mkdir -p $out/share/gsettings-schemas
    ln -s ${fp6-privacy-schema}/share/gsettings-schemas/${fp6-privacy-schema.name} $out/share/gsettings-schemas/
    runHook postInstall
  '';
  meta = {
    description = "Emergency information, SOS, lock grace, clipboard and Wi-Fi MAC pages for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
