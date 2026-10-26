# Reviewed additional source ports. No services or hardware operations are enabled.
{ pkgs }:
let
  lpa-gtk = pkgs.callPackage ./postmarketos-apps-lpa-gtk.nix {};
  vvmplayer = pkgs.callPackage ./postmarketos-apps-vvmplayer.nix {};
  phosh-tour = pkgs.callPackage ./postmarketos-apps-phosh-tour.nix {};
in {
  inherit lpa-gtk vvmplayer phosh-tour;
  packages = [ lpa-gtk vvmplayer phosh-tour ];
  pending = [ "chatty" ]; # Mandatory libcmatrix/olm requires separate security review.
}
