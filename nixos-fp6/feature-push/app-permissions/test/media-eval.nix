# NixOS module evaluation of the camera / microphone / location option on
# the full kernel6 target, off (as shipped) and on:
#   nix-instantiate --eval --strict -E 'import ./test/media-eval.nix { }'
# Evaluates only (no build); throws on the first failed check.
{ target ? ../../trial-lomiri-full-kernel6-fp6.nix }:
let
  t = import target { };
  lib = (import (import ../../../upstream/mobile-nixos/npins).nixpkgs { }).lib;
  with' = on: (t.eval.extendModules {
    modules = [ { hardware.fairphone6.appPermissions.media.enable = lib.mkForce on; } ];
  }).config;
  off = with' false;
  on = with' true;
  cfgOf = c: c.system.build.fp6-app-permissions.brokerSettings;
  bOff = cfgOf off;
  bOn = cfgOf on;
  check = ok: msg: if ok then true else throw "media-eval: FAILED: ${msg}";
  checks = [
    (check (bOff.media == [ ] && bOff.defaults == { }) "off: broker has no media permissions")
    (check (!(off.systemd.user.services ? fp6-media-gate)) "off: no media gate")
    (check (!(off.services.pipewire.wireplumber.extraScripts ? "fp6-media-permissions.lua")) "off: no WirePlumber script")
    (check ((off.systemd.user.services.pipewire-pulse.serviceConfig.ExecStart or null) == null) "off: stock pipewire-pulse")
    (check (!(lib.hasInfix "geoclue-fp6" (toString (off.systemd.services.geoclue.serviceConfig.ExecStart or "")))) "off: stock GeoClue")

    (check (bOn.media == [ "camera" "microphone" "location" ]) "on: camera, microphone, location enforced")
    (check (bOn.defaults.lomiri-camera-app == { camera = "allow"; microphone = "allow"; location = "while_using"; }) "on: Camera app presets")
    (check (bOn.defaults.lomiri-weather-app.location == "while_using") "on: Weather location preset")
    (check (bOn.defaults."io.github.rinigus.PureMaps".location == "while_using") "on: Maps location preset")
    (check (bOn.defaults.firefox.location == "allow") "on: GeoClue-allowed apps keep location")
    (check (lib.all (a: lib.elem a bOn.system_apps) [ "lomiri-dialer-app" "lomiri-messaging-app" "fp6-dictation" "fp6-captions" ]) "on: Phone, Messaging, dictation, captions are system apps")
    (check (lib.elem "fp6-media-gate.service" bOn.trusted_units && lib.elem "fp6-location-indicator.service" bOn.trusted_units) "on: gate and location agent are trusted daemons")
    (check (on.systemd.user.services ? fp6-media-gate) "on: media gate service")
    (check (on.systemd.user.services.fp6-media-gate.environment.FP6_MEDIA_PERMS == "camera microphone") "on: gate gates camera and microphone")
    (check (on.services.pipewire.wireplumber.extraScripts ? "fp6-media-permissions.lua") "on: WirePlumber script")
    (check (lib.hasSuffix "/bin/pipewire-pulse" (lib.last on.systemd.user.services.pipewire-pulse.serviceConfig.ExecStart)) "on: patched pipewire-pulse")
    (check (lib.hasSuffix "/libexec/geoclue" (lib.last on.systemd.services.geoclue.serviceConfig.ExecStart)) "on: patched GeoClue")
    (check (on.systemd.user.services.fp6-location-indicator.environment.FP6_LOCATION_PERMISSIONS == "1") "on: location agent asks fp6-permd")
  ];
in builtins.deepSeq checks "media-eval: ${toString (builtins.length checks)} checks passed"
