# One contacts store for every Lomiri app and daemon on the FP6
# (results/feature-push-20261024/contacts/README.md).
#
# Store: qtcontacts-sqlite ("org.nemomobile.contacts.sqlite",
# ~/.local/share/system/privileged/Contacts/qtcontacts-sqlite/contacts.db).
# Current Ubuntu Touch moved off address-book-service ("galera", on top of EDS),
# which nixpkgs does not package: lomiri-addressbook-app (pinned main) already
# defaults to qtcontacts-sqlite, lomiri-history-service 0.7 reads
# LOMIRI_CONTACT_BACKEND (default still "galera"), and lomiri-telephony-service
# main defaults to qtcontacts-sqlite (backported in
# cellular/telephony-service-contacts-fallback.patch).
#
# This module
#  * builds Contacts, Dialer and Messages against the patched telephony-service
#    (its Lomiri.Telephony QML plugin: ContactWatcher, call/thread names) and
#    sets LOMIRI_CONTACT_BACKEND in their wrappers (Lomiri.History models);
#  * gives lomiri-history-daemon the backend and the qtcontacts-sqlite plugin
#    (participant names stored with threads/events);
#  * lets the confined Messages app open the contacts database.
# The telephony daemons (notifications, approver, handler) get the same build
# from cellular/messaging-fp6.nix.
{ config, lib, pkgs, ... }:
let
  frontends = import ../../telephony-frontends-packages.nix { inherit pkgs; };
  telephony-service = import ../cellular/telephony-service-fp6.nix { inherit pkgs lib; };
  backend = telephony-service.contactsBackend;
  qtcontactsPlugins = "${frontends.qtcontacts-sqlite}/${pkgs.libsForQt5.qtbase.qtPluginPrefix}";

  withBackend = drv: drv.overrideAttrs (old: {
    preConfigure = (old.preConfigure or "") + ''
      qtWrapperArgs+=(--set-default LOMIRI_CONTACT_BACKEND ${backend})
    '';
  });
  addressbook = withBackend (frontends.addressbook.override {
    lomiri-telephony-service = telephony-service;
  });
  frontend = drv: withBackend (drv.override {
    lomiri-telephony-service = telephony-service;
    inherit addressbook;
  });
  dialer = frontend frontends.dialer;
  messaging = frontend frontends.messaging;
in {
  config = lib.mkIf config.programs.fp6TelephonyFrontends.enable {
    programs.fp6TelephonyFrontends.packages = lib.mkForce [ addressbook dialer messaging ];

    # The unit only reaches the session through sw/share/systemd/user. List
    # the package in systemd.packages so the settings below become a drop-in
    # on its unit (otherwise NixOS writes an /etc unit without ExecStart).
    # The wrapper prefixes its own qtpim/qtbase plugin dirs to QT_PLUGIN_PATH.
    systemd.packages = [ pkgs.lomiri.lomiri-history-service ];
    systemd.user.services.lomiri-history-daemon.environment = {
      LOMIRI_CONTACT_BACKEND = backend;
      QT_PLUGIN_PATH = qtcontactsPlugins;
    };

    # Messages runs under the enforced lomiri-messaging-app profile, which
    # includes this file. qtcontacts-sqlite falls back to an empty
    # nonprivileged database when it cannot create/open the privileged one,
    # so grant exactly the privileged store (database, journal/WAL, lock file).
    security.apparmor.includes."local/lomiri-messaging-app" = lib.mkIf config.security.apparmor.enable ''
      owner @{HOME}/.local/share/system/ r,
      owner @{HOME}/.local/share/system/privileged/ r,
      owner @{HOME}/.local/share/system/privileged/Contacts/ rw,
      owner @{HOME}/.local/share/system/privileged/Contacts/qtcontacts-sqlite/ rw,
      owner @{HOME}/.local/share/system/privileged/Contacts/qtcontacts-sqlite/** rwk,
    '';

    system.build.fp6-contacts = {
      inherit telephony-service addressbook dialer messaging;
      lookupProbe = import ./lookup-probe.nix { inherit pkgs lib; inherit (frontends) qtcontacts-sqlite; };
    };
  };
}
