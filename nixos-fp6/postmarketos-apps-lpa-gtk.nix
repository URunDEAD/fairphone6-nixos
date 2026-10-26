# Source port only; never enables a modem service or performs an eSIM operation.
{ lib, stdenvNoCC, fetchurl, meson, ninja, pkg-config, blueprint-compiler
, desktop-file-utils, wrapGAppsHook4, gtk4, libadwaita, python3, lpac }:
let python = python3.withPackages (p: [ p.pygobject3 ]);
in stdenvNoCC.mkDerivation {
  pname = "lpa-gtk";
  version = "0.4";
  src = fetchurl {
    url = "https://codeberg.org/lucaweiss/lpa-gtk/archive/0.4.tar.gz";
    sha256 = "06ee0ce2c891f90c30dbfe0a804fdf319d2648ac619c25159f16e1caf7ccec2d";
  };
  nativeBuildInputs = [ meson ninja pkg-config blueprint-compiler desktop-file-utils wrapGAppsHook4 python ];
  buildInputs = [ gtk4 libadwaita ];
  preFixup = ''
    gappsWrapperArgs+=(--prefix PATH : ${lib.makeBinPath [ lpac ]})
  '';
  doCheck = true;
  meta = {
    description = "eSIM Manager frontend; hardware/SIM activation requires separate acceptance";
    homepage = "https://codeberg.org/lucaweiss/lpa-gtk";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "lpa-gtk";
  };
}
