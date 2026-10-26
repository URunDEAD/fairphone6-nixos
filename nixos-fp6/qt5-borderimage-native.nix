# Complete native Qt5 integration. Import instead of qt5-borderimage-runtime.nix.
# The Qt patch package is already built; its Qt5 module/app consumers rebuild
# with their own final prefixes, including paths compiled as QStringLiteral.
{ lib, ... }:
{
  nixpkgs.overlays = lib.mkAfter [ (final: prev: {
    qt5 = prev.qt5.overrideScope (_qfinal: _qprev: {
      qtdeclarative = import ./qt5-borderimage/default.nix;
    });
  }) ];
}
