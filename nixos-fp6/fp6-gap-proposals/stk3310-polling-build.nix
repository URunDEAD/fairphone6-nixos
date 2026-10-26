# Build-only diagnostic; never installs/loads itself into the system target.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../../kernel-build
, eventFree ? false
}:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  kernel = import kernelBuild { inherit nixpkgs; system = "aarch64-linux"; };
  release = kernel.modDirVersion;
  llvm = pkgs.llvmPackages;
in assert release == "7.2.0-fp6-nixos1";
llvm.stdenv.mkDerivation {
  pname = "fp6-stk3310-polling-diagnostic";
  version = if eventFree then "7.2.0-2" else "7.2.0-1";
  src = kernel.src;
  nativeBuildInputs = kernel.moduleBuildDependencies ++ [ llvm.lld llvm.llvm pkgs.kmod ];
  hardeningDisable = [ "pic" ];
  unpackPhase = ''
    runHook preUnpack
    mkdir -p source/drivers/iio/light
    tar -xOf "$src" linux-63289f4411520a2acb38bd533437446d33fbce74/drivers/iio/light/stk3310.c \
      > source/drivers/iio/light/stk3310.c
    cd source
    printf '%s\n' 'obj-m := stk3310.o' > drivers/iio/light/Makefile
    runHook postUnpack
  '';
  patches = [ (if eventFree then ./stk3310-noirq-events.patch else ./stk3310-noirq.patch) ];
  buildPhase = ''
    runHook preBuild
    make -C ${kernel.dev}/lib/modules/${release}/build \
      M="$PWD/drivers/iio/light" LLVM=1 \
      CC=${llvm.clang-unwrapped}/bin/clang LD=${llvm.lld}/bin/ld.lld \
      AR=${llvm.llvm}/bin/llvm-ar NM=${llvm.llvm}/bin/llvm-nm \
      STRIP=${llvm.llvm}/bin/llvm-strip OBJCOPY=${llvm.llvm}/bin/llvm-objcopy \
      OBJDUMP=${llvm.llvm}/bin/llvm-objdump READELF=${llvm.llvm}/bin/llvm-readelf \
      HOSTCC=${llvm.clang}/bin/clang HOSTCXX=${llvm.clang}/bin/clang++ \
      SHELL=${llvm.stdenv.shell} modules
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    test "$(modinfo -F vermagic drivers/iio/light/stk3310.ko | cut -d' ' -f1)" = ${release}
    llvm-readelf -h drivers/iio/light/stk3310.ko | grep 'Machine:.*AArch64'
    modinfo drivers/iio/light/stk3310.ko
    runHook postCheck
  '';
  installPhase = ''
    runHook preInstall
    install -Dm644 drivers/iio/light/stk3310.ko \
      "$out/lib/modules/${release}/extra/stk3310.ko"
    mkdir -p "$out/share/fp6-stk3310-diagnostic"
    cp drivers/iio/light/stk3310.c "$out/share/fp6-stk3310-diagnostic/"
    modinfo drivers/iio/light/stk3310.ko > "$out/share/fp6-stk3310-diagnostic/modinfo.txt"
    runHook postInstall
  '';
  dontStrip = true;
  meta = {
    description = "Unactivated FP6 STK3310/STK36C61 no-IRQ polling diagnostic module";
    license = pkgs.lib.licenses.gpl2Only;
    platforms = [ "aarch64-linux" ];
  };
}
