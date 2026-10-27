# FP6 call recording, dialer side: Record button + REC indicator in the call
# screen, Settings > Call recording, the Recordings list and a "Call
# recordings" header action on the dialer page.
#   dialer = import ./dialer/dialer.nix { inherit lib pkgs schema; } dialer;
# `schema` holds the compiled com.fairphone6.callrecording schema under
# $out/share/gsettings-schemas/$name/glib-2.0/schemas (../schema.nix).
{ lib, pkgs, schema }:
dialer: dialer.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./lomiri-dialer-app-call-recording.patch ];

  # The QML dirs install with file(GLOB *.qml), so new files only need to be
  # in the source tree before configure. CallRecorder/CallRecordingNotice sit
  # next to dialer-app.qml: LiveCallPage and DialerPage import "../".
  postPatch = (old.postPatch or "") + ''
    install -m644 ${./CallRecorder.qml} src/qml/CallRecorder.qml
    install -m644 ${./CallRecordingNotice.qml} src/qml/CallRecordingNotice.qml
    install -m644 ${./CallRecordingSettings.qml} src/qml/SettingsPage/CallRecordingSettings.qml
    install -m644 ${./CallRecordingContactPicker.qml} src/qml/SettingsPage/CallRecordingContactPicker.qml
    install -m644 ${./RecordingsPage.qml} src/qml/SettingsPage/RecordingsPage.qml
  '';

  # QMenuModel (QDBusActionGroup for the fp6-callrec actions). wrapQtAppsHook
  # adds lib/qt-*/qml of every buildInput to the wrapper's QML2_IMPORT_PATH.
  buildInputs = (old.buildInputs or [ ]) ++ [ pkgs.libsForQt5.qmenumodel ];

  preConfigure = (old.preConfigure or "") + ''
    qtWrapperArgs+=(--prefix XDG_DATA_DIRS : ${schema}/share/gsettings-schemas/${schema.name})
  '';
})
