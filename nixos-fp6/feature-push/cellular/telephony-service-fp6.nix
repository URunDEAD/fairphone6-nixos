# lomiri-telephony-service 0.6.2 with the FP6 contacts patch
# (telephony-service-contacts-fallback.patch): the shared contacts lookup
# (ContactUtils::sharedManager, used by the daemons and by the Lomiri.Telephony
# QML plugin the Messages/Dialer/Contacts apps load) uses qtcontacts-sqlite
# instead of the unpackaged "galera" engine, as upstream main does.
# Shared by cellular/messaging-fp6.nix (daemons) and contacts/contacts-fp6.nix
# (apps), so both run the same build.
{ pkgs, lib }:
let
  frontends = import ../../telephony-frontends-packages.nix { inherit pkgs; };
  backend = "org.nemomobile.contacts.sqlite";
  qtPlugins = lib.makeSearchPathOutput "bin" pkgs.libsForQt5.qtbase.qtPluginPrefix [
    frontends.qtcontacts-sqlite
    pkgs.libsForQt5.qtpim
    pkgs.libsForQt5.qtbase
  ];
in pkgs.lomiri.lomiri-telephony-service.overrideAttrs (old: {
  pname = "lomiri-telephony-service-fp6-daemons";
  patches = (old.patches or [ ]) ++ [ ./telephony-service-contacts-fallback.patch ];
  nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];
  postFixup = (old.postFixup or "") + ''
    for prog in lomiri-telephony-service-approver lomiri-telephony-service-handler lomiri-indicator-telephony-service; do
      wrapProgram "$out/bin/$prog" --prefix QT_PLUGIN_PATH : ${qtPlugins} \
        --set-default LOMIRI_CONTACT_BACKEND ${backend}
    done
  '';
  passthru = (old.passthru or { }) // { contactsBackend = backend; };
})
