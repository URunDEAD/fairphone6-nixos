# SMS user-visible path fixes for the Lomiri messaging stack on FP6
# (results/feature-push-20261024/cellular/messaging-README.md).
#
# 1. Incoming SMS raised no notification popup and no messaging-menu entry
#    (only the history-service launcher badge). lomiri-telephony-service's
#    indicator looks the sender up in ContactUtils::sharedManager(), which is
#    hard-wired to the QtContacts "galera" engine (address-book-service, not
#    packaged here). With no such engine (and no QT_PLUGIN_PATH for any
#    contacts engine at all) QtContacts returns its invalid engine, whose
#    QContactFetchRequest::start() fails; the request never reaches
#    FinishedState, so showNotificationForMessage()/MessagingMenu::addMessage()
#    are never called. Fix: fall back to qtcontacts-sqlite (the engine the
#    address book app stores contacts in) or "memory", never wait on a request
#    that failed to start, and give the three telephony-service daemons a
#    QT_PLUGIN_PATH with the contacts engines.
#    Only the daemons (indicator, approver, handler) run the patched build;
#    the global lomiri-telephony-service (QML plugin used by the apps) stays
#    unchanged so the frontends and the shell are not rebuilt.
# 2. telepathy-ofono: "QSQLITE driver not loaded" (it is dontWrapQtApps and
#    D-Bus activated without QT_PLUGIN_PATH), so its pending-message /
#    MMS-group database never opened. Wrap it with qtbase's plugins.
{ config, lib, pkgs, options, ... }:
let
  frontends = import ../../telephony-frontends-packages.nix { inherit pkgs; };
  qtPlugins = lib.makeSearchPathOutput "bin" pkgs.libsForQt5.qtbase.qtPluginPrefix [
    frontends.qtcontacts-sqlite
    pkgs.libsForQt5.qtpim
    pkgs.libsForQt5.qtbase
  ];
  telephony-service = pkgs.lomiri.lomiri-telephony-service.overrideAttrs (old: {
    pname = "lomiri-telephony-service-fp6-daemons";
    patches = (old.patches or [ ]) ++ [ ./telephony-service-contacts-fallback.patch ];
    nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];
    postFixup = (old.postFixup or "") + ''
      for prog in lomiri-telephony-service-approver lomiri-telephony-service-handler lomiri-indicator-telephony-service; do
        wrapProgram "$out/bin/$prog" --prefix QT_PLUGIN_PATH : ${qtPlugins}
      done
    '';
  });
  telepathyOfonoDefault = options.services.fp6TelephonyMiddleware.package.default;
  telepathy-ofono = telepathyOfonoDefault.overrideAttrs (old: {
    nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.makeWrapper ];
    postFixup = (old.postFixup or "") + ''
      wrapProgram "$out/libexec/telepathy-ofono" \
        --prefix QT_PLUGIN_PATH : ${lib.makeSearchPathOutput "bin" pkgs.libsForQt5.qtbase.qtPluginPrefix [ pkgs.libsForQt5.qtbase ]}
    '';
  });
  daemon = name: {
    serviceConfig.ExecStart = [ "" "${telephony-service}/bin/${name}" ];
  };
in {
  config = lib.mkIf config.services.fp6Cellular.enable {
    services.fp6TelephonyMiddleware.package = telepathy-ofono;

    systemd.user.services.lomiri-indicator-telephony-service = daemon "lomiri-indicator-telephony-service";
    systemd.user.services.lomiri-telephony-service-approver = daemon "lomiri-telephony-service-approver";
    systemd.user.services.lomiri-telephony-service-handler = daemon "lomiri-telephony-service-handler";

    system.build.fp6-messaging = { inherit telephony-service telepathy-ofono; };
  };
}
