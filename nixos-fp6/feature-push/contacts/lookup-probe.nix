# Test helper for results/feature-push-20261024/contacts/README.md (not part of
# the system closure): fp6-contacts-lookup-probe resolves a number through
# lomiri-history-service's ContactMatcher. Its private header is taken from the
# same history-service source that nixpkgs builds.
{ pkgs, lib, qtcontacts-sqlite }:
let
  qt = pkgs.libsForQt5;
  history = pkgs.lomiri.lomiri-history-service;
in pkgs.stdenv.mkDerivation {
  pname = "fp6-contacts-lookup-probe";
  version = "1";
  src = ./lookup-probe.cpp;
  dontUnpack = true;
  nativeBuildInputs = [ qt.wrapQtAppsHook pkgs.pkg-config ];
  buildInputs = [ qt.qtbase qt.qtpim history ];
  buildPhase = ''
    cp ${history.src}/src/contactmatcher_p.h .
    $CXX -std=c++17 -fPIC -o fp6-contacts-lookup-probe $src -I. \
      $(pkg-config --cflags --libs Qt5Core Qt5Contacts) \
      -L${history}/lib -llomirihistoryservice
  '';
  installPhase = ''
    install -Dm755 fp6-contacts-lookup-probe $out/bin/fp6-contacts-lookup-probe
  '';
  qtWrapperArgs = [ "--prefix" "QT_PLUGIN_PATH" ":" "${qtcontacts-sqlite}/${qt.qtbase.qtPluginPrefix}" ];
}
