{ stdenvNoCC
# Start the boot animation at the glow loop (the resting logo) instead of the
# spin-in intro. The FP6 boot splash sets this when the stage-1 bridge
# frame already shows the resting logo (see ../boot-splash-fp6.nix).
, bootStartsAtLoop ? false
}:

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
  '' + (if bootStartsAtLoop then ''
    substituteInPlace $dir/nixos-mobile.script --replace-fail "BOOT_START = 0;" "BOOT_START = N_INTRO;"
  '' else "") + ''
    runHook postInstall
  '';
}
