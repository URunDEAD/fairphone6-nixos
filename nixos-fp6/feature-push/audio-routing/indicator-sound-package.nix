# ayatana-indicator-sound 24.5.3 (latest release; upstream main has no device
# selection either) plus Output/Input sections in the sound menu.
#
# The patch adds src/audio-device-selector.vala (one choice per card port,
# card profile switched when the port needs it) and the actions
#   indicator.output-device / indicator.input-device  (s, radio state = id)
#   indicator.output-devices / indicator.input-devices (JSON list for Settings)
# It also installs libexec/ayatana-indicator-sound/fp6-audio-devices-dump, a
# read-only diagnostic that prints the same lists and exits.
#
# Upstream tests are off: indicator-test asserts the exact old menu layout
# and its LD_PRELOAD pulse mock has no card API.
{ ayatana-indicator-sound, vala, pkg-config, libgee, libpulseaudio, glib }:
ayatana-indicator-sound.overrideAttrs (old: {
  pname = "ayatana-indicator-sound-fp6";
  # indicator-sound-restore-codec.patch (bt-codecs feature): after the
  # indicator's own call routing, return a headset to the exact A2DP profile
  # it had before the call (e.g. a chosen SBC), else the old priority pick.
  patches = (old.patches or [ ]) ++ [ ./indicator-sound-device-selection.patch ./indicator-sound-restore-codec.patch ];
  doCheck = false;
  postInstall = (old.postInstall or "") + ''
    mkdir -p $out/libexec/ayatana-indicator-sound
    valac --vapidir ../vapi --pkg fp6-pulse-ports --pkg gee-0.8 --pkg libpulse --pkg libpulse-mainloop-glib --pkg gio-2.0 \
      -X -DGETTEXT_PACKAGE=\"ayatana-indicator-sound\" -X -Wno-incompatible-pointer-types -X -Wno-discarded-qualifiers \
      -o $out/libexec/ayatana-indicator-sound/fp6-audio-devices-dump \
      ../src/audio-device-selector.vala ${./audio-devices-dump.vala}
  '';
  passthru = (old.passthru or { }) // { fp6DeviceSelection = true; };
})
