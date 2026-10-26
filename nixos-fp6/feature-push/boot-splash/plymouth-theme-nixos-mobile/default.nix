{ stdenvNoCC }:

stdenvNoCC.mkDerivation {
  pname = "plymouth-theme-nixos-mobile";
  version = "1.0";
  src = ./.;

  installPhase = ''
    runHook preInstall
    dir=$out/share/plymouth/themes/nixos-mobile
    mkdir -p $dir
    cp images/*.png nixos-mobile.script $dir/
    substitute nixos-mobile.plymouth $dir/nixos-mobile.plymouth --replace-fail "@out@" "$dir"
    runHook postInstall
  '';
}
