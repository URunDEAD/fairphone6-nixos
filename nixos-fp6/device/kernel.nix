{ lib, stdenv, stdenvNoCC, features ? {}, ... }:
stdenvNoCC.mkDerivation {
  pname = "linux-fp6-catcrafts";
  version = "7.2.0";
  src = ../assets/fp6-kernel.tar;
  dontUnpack = true;
  outputs = [ "out" "dev" ];
  installPhase = ''
    mkdir unpack
    tar xf $src -C unpack
    mkdir -p $out/lib $out/dtbs/qcom $dev/lib/modules/7.2.0/build
    cp unpack/boot/vmlinuz $out/Image
    cp unpack/boot/milos-fairphone-fp6.dtb $out/dtbs/qcom/
    cp -a unpack/usr/lib/modules $out/lib/
    cp unpack/home/user/fp6-install/kernel-7.2.0.config $out/config
    cp $out/config $dev/lib/modules/7.2.0/build/.config
    # Module build links refer to the original Alpine build tree.
    rm -f $out/lib/modules/7.2.0/build $out/lib/modules/7.2.0/source
  '';
  passthru = rec {
    inherit stdenv;
    baseVersion = "7.2.0";
    modDirVersion = "7.2.0";
    file = "Image";
    target = "Image";
    kernelOlder = lib.versionOlder baseVersion;
    kernelAtLeast = lib.versionAtLeast baseVersion;
    isLTS = false;
    isZen = false;
    isQcdt = false;
    isExynosDT = false;
    commonMakeFlags = [];
    configfile = ../assets/kernel.config;
    features = { efiBootStub = true; };
    config = {
      isEnabled = name:
        let lines = lib.splitString "\n" (builtins.readFile ../assets/kernel.config);
        in builtins.elem "CONFIG_${name}=y" lines || builtins.elem "CONFIG_${name}=m" lines;
    };
  };
  meta.platforms = [ "aarch64-linux" ];
}
