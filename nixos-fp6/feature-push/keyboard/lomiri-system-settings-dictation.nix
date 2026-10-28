# System Settings > Voice Dictation: pure-QML plugin (manifest + page) with
# the switch for the keyboard's microphone key (org.maliit.keyboard.maliit
# dictation-key, maliit-keyboard-dictation-key.patch). That schema reaches
# lomiri-system-settings through its maliit-keyboard build input.
{ lib, stdenvNoCC }:
stdenvNoCC.mkDerivation {
  pname = "lomiri-system-settings-voice-dictation";
  version = "0.1.0";
  src = ./settings-page;
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm644 voice-dictation.settings $out/share/lomiri-system-settings/voice-dictation.settings
    install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/voice-dictation PageComponent.qml
    # The wrapper lndir's these from every plugin.
    mkdir -p $out/lib/lomiri-system-settings $out/share/locale
    runHook postInstall
  '';
  meta = {
    description = "Voice Dictation page (keyboard microphone key) for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
