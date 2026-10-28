# The bundled lock/unlock sounds: short clips from sound themes that are
# already in nixpkgs, converted at build time by fp6-lock-sounds (trimmed,
# peak -9 dBFS, 16-bit WAV for SoundEffect). Licences: CC-BY-SA, credited
# in the bundle's CREDITS file and results/feature-push-20261024/lock-sounds/README.md.
{ sound-theme-freedesktop, ocean-sound-theme, lomiri-sounds }:
let
  fd = "${sound-theme-freedesktop}/share/sounds/freedesktop/stereo";
  ocean = "${ocean-sound-theme}/share/sounds/ocean/stereo";
  lomiri = "${lomiri-sounds}/share/sounds/lomiri";
  borealis = {
    credit = "Ivica Bukvic, Borealis sound theme (sound-theme-freedesktop 0.8, CREDITS)";
    license = "CC-BY-SA";
  };
  oceanCredit = {
    credit = "Guilherme Marçal Silva, Ocean sound theme (KDE Plasma)";
    license = "CC-BY-SA-4.0";
  };
in [
  ({ id = "pop-down";   name = "Pop down";   src = "${fd}/device-removed.oga"; } // borealis)
  ({ id = "pop-up";     name = "Pop up";     src = "${fd}/device-added.oga"; } // borealis)
  ({ id = "click";      name = "Click";      src = "${ocean}/button-pressed.oga"; } // oceanCredit)
  ({ id = "soft-click"; name = "Soft click"; src = "${ocean}/button-pressed-modifier.oga"; } // oceanCredit)
  ({ id = "tick";       name = "Tick";       src = "${fd}/dialog-information.oga"; } // borealis)
  { id = "tap";         name = "Tap";        src = "${fd}/audio-volume-change.oga";
    credit = "Lucas McCallister (sound-theme-freedesktop 0.8, CREDITS)"; license = "CC-BY-SA"; }
  ({ id = "chime";      name = "Chime";      src = "${ocean}/completion-success.oga"; } // oceanCredit)
  ({ id = "chime-low";  name = "Low chime";  src = "${ocean}/completion-partial.oga"; } // oceanCredit)
  { id = "slick";       name = "Slick";      src = "${lomiri}/notifications/Slick.ogg";
    credit = "Canonical Ltd., lomiri-sounds (Ubuntu Touch)"; license = "CC-BY-SA-3.0"; }
]
