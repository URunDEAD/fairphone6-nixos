# Clean external rebuild of the whole CAMSS module against exact kernel4.
# Original Image/modules outputs remain immutable; depmod selects updates/.
{ lib, runCommand, llvmPackages_21, kernel
# Kernel5 targets pass their own pin; the defaults keep the kernel4 derivation.
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
}:
assert kernel.drvPath == expectedKernelDrvPath;
assert kernel.modDirVersion == kernelRelease;
assert builtins.hashFile "sha256" ./selfie-camss-kernel4/milos-dphy-no-cphy-rate-table.patch
  == "03dfd28758f2a06c39bd361df308d4c3be483d66fea2a6d61b07da1ab1c565ec";
assert builtins.hashFile "sha256" ./selfie-camss-kernel4/csid340-phy-select-three-bits.patch
  == "a79f3660754a15fbf22c4aa974516940a726433d1aa01405227c8a3b26f40319";
let
  source = runCommand "fp6-camss-exact-archive-source" {} ''
    echo '1a1aba170899273cbb97bce2dc0713957340a7be2e2d187faab6f821d60687ba  ${kernel.src}' | sha256sum -c -
    mkdir -p "$out/drivers/media/platform/qcom/camss"
    tar -xzf ${kernel.src} --strip-components=1 -C "$out" \
      --wildcards '*/drivers/media/platform/qcom/camss/*'
  '';
in llvmPackages_21.stdenv.mkDerivation {
  pname = "fp6-camss-kernel4-csid-phy3";
  version = "1";
  src = source;
  patches = [
    ./selfie-camss-kernel4/milos-dphy-no-cphy-rate-table.patch
    ./selfie-camss-kernel4/csid340-phy-select-three-bits.patch
  ];
  nativeBuildInputs = kernel.moduleBuildDependencies ++ [ llvmPackages_21.lld llvmPackages_21.llvm ];
  dontConfigure = true;
  dontStrip = true;
  dontPatchELF = true;
  buildPhase = ''
    runHook preBuild
    headers=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build
    test "$(cat "$headers/include/config/kernel.release")" = "${kernelRelease}"
    test -s "$headers/Module.symvers"
    test "$(grep '^CONFIG_VIDEO_QCOM_CAMSS=' "$headers/.config")" = "CONFIG_VIDEO_QCOM_CAMSS=m"
    make -C "$headers" M="$PWD/drivers/media/platform/qcom/camss" -j2 LLVM=1 \
      CC=${llvmPackages_21.clang}/bin/clang \
      LD=${llvmPackages_21.lld}/bin/ld.lld \
      AR=${llvmPackages_21.llvm}/bin/llvm-ar \
      NM=${llvmPackages_21.llvm}/bin/llvm-nm \
      OBJCOPY=${llvmPackages_21.llvm}/bin/llvm-objcopy \
      STRIP=${llvmPackages_21.llvm}/bin/llvm-strip \
      KCFLAGS=-Wno-unused-command-line-argument modules
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm644 drivers/media/platform/qcom/camss/qcom-camss.ko \
      "$out/lib/modules/${kernel.modDirVersion}/updates/qcom-camss.ko"
    mkdir -p "$out/etc/depmod.d"
    echo 'override qcom_camss ${kernelRelease} updates' > "$out/etc/depmod.d/50-fp6-camss.conf"
    runHook postInstall
  '';
  passthru = {
    kernelRelease = kernel.modDirVersion;
    kernelDrv = kernel.drvPath;
    moduleRelativePath = "lib/modules/${kernel.modDirVersion}/updates/qcom-camss.ko";
    automaticSensorModuleLoadingAdded = false;
  };
  meta = {
    description = "Exact kernel4 CAMSS module with official DPHY sequence and three-bit PHY selector";
    license = lib.licenses.gpl2Only;
    platforms = [ "aarch64-linux" ];
  };
}
