# The same0.3 read-only diagnostics, rebuilt for the separate DPHY kernel4.
# Never force-load the kernel3 module or install into an autoload search path.
{ lib, llvmPackages_21, kernel
# Kernel5 targets pass their own pin; the defaults keep the kernel4 derivation.
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
}:
assert kernel.drvPath == expectedKernelDrvPath;
assert kernel.modDirVersion == kernelRelease;
assert builtins.hashFile "sha256" ./selfie-fp6-diagnostic/fp6-s5kkd1.c == "65b05a05bd284f65d9aef25e5e68af259ddf2b0d462920e66fb2c05fd99282e9";
llvmPackages_21.stdenv.mkDerivation {
  pname = "fp6-s5kkd1-driver-diagnostic";
  version = "0.3-kernel4-stream-reads";
  src = ./selfie-fp6-diagnostic;
  nativeBuildInputs = kernel.moduleBuildDependencies ++ [ llvmPackages_21.lld llvmPackages_21.llvm ];
  dontConfigure = true;
  dontStrip = true;
  dontPatchELF = true;
  buildPhase = ''
    runHook preBuild
    headers=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build
    test "$(cat "$headers/include/config/kernel.release")" = "${kernelRelease}"
    test -s "$headers/Module.symvers"
    make -C "$headers" M="$PWD" -j2 LLVM=1 \
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
    install -Dm644 fp6-s5kkd1.ko "$out/share/fp6-selfie-diagnostic/fp6-s5kkd1.ko"
    for file in fp6-s5kkd1.c fp6-s5kkd1-firmware.h; do
      install -Dm644 "$file" "$out/share/fp6-selfie-diagnostic/$file"
    done
    runHook postInstall
  '';
  passthru = {
    kernelRelease = kernel.modDirVersion;
    kernelDrv = kernel.drvPath;
    moduleRelativePath = "share/fp6-selfie-diagnostic/fp6-s5kkd1.ko";
    defaultStreamingEnabled = false;
    diagnosticRegisters = [ "0x0005" "0x0100" "0x0101" ];
  };
  meta = {
    description = "Explicit-load FP6 selfie0.3 read-only stream diagnostics for the separate exact DPHY kernel4";
    license = lib.licenses.gpl2Only;
    platforms = [ "aarch64-linux" ];
  };
}
