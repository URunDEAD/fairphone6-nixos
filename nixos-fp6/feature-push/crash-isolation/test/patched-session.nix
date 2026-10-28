# lomiri-session's run-systemd-session with the patches the phone applies, in
# order: configuration.nix (lomiri-session-wayland-grid), lomiri-session/
# (session-end, overlay 2300), crash-isolation (shell-restart, overlay 4100).
# Proves the patch applies; shell-restart-test.sh runs the result.
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
pkgs.runCommand "fp6-run-systemd-session-patched" { nativeBuildInputs = [ pkgs.patch ]; } ''
  cp -r ${pkgs.lomiri.lomiri-session.src} src
  chmod -R u+w src
  cd src
  patch -p1 < ${../../../lomiri-session-wayland-grid.patch}
  patch -p1 < ${../../lomiri-session/lomiri-session-session-end.patch}
  patch -p1 < ${../lomiri-session-shell-restart.patch}
  sh -n systemd/run-systemd-session
  install -Dm755 systemd/run-systemd-session $out/run-systemd-session
''
