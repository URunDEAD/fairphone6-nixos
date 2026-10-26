# FP6 MMS fixes on top of ../../mms-fp6.nix (nuntium user service).
#
# Transport needs nothing extra: upstream oFono 2.19 brings the type=mms
# context up on its own rmnet_dataN (/32 address) and adds a host route to the
# MessageProxy through it, so the HTTP POST via the Orange RO proxy
# (62.217.247.252:8799, MMSC http://wap.mms.orange.ro:8002, APN "mms",
# provisioned from mobile-broadband-provider-info) bypasses WiFi's default route.
#
# nuntium-fp6-mms-fixes.patch:
#  * ofono.Modem.watchStatus dereferenced a nil *dbus.Signal after Delete()
#    closed the channel -> nuntium panicked on every oFono restart/modem removal
#    (StartLimitBurst=3 in 60 s could then leave MMS dead until next login).
#  * log the MMSC's X-Mms-Response-Text next to ResponseStatus (diagnostics).
#  * getContextProperties logged "Cannot get properties ... <nil>" on success.
{ config, lib, pkgs, ... }:
let
  base = pkgs.callPackage ../../nuntium-package.nix { };
in {
  config = lib.mkIf (config.services.fp6Cellular.enable && config.services.fp6Cellular.mms) {
    services.fp6Mms.package = base.overrideAttrs (old: {
      patches = (old.patches or [ ]) ++ [ ./nuntium-fp6-mms-fixes.patch ];
    });
  };
}
