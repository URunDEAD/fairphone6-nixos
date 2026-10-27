# Compile-only check of the kernel7 CS-call-audio candidates against the
# exact kernel6 headers (7.2.0-fp6-nixos6), as EXTERNAL objects/modules. It
# never builds a kernel (inputs via builtins.storePath, like compile-check.nix).
#   0005 q6apm-dai hostless front ends     -> snd-q6apm + q6apm-dai rebuilt as modules
#   0006 audioreach generic module config  -> (same modules)
#   0007 net: qrtr forwarding (Catcrafts)   -> af_qrtr.o / ns.o objects only
#        (QRTR is built in, so no module/modpost for it)
# The .ko files are evidence only (renamed *_fp6check); never load them.
#
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file kernel-build/feature-push/kernel7-candidates/cs-call-audio-compile-check.nix --no-link --print-out-paths
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }
, kernelSrc ? "/nix/store/i9iynws42nf1a5dcrvwvibj8q91czcwd-linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz"
, kernelDev ? "/nix/store/3disx214wj8xd00imh7b83a58fa15jf0-linux-fp6-nixos-7.2.0-dev"
, release ? "7.2.0-fp6-nixos6"
}:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  llvm = pkgs.llvmPackages;
  src = builtins.storePath kernelSrc;
  dev = builtins.storePath kernelDev;
  top = "linux-63289f4411520a2acb38bd533437446d33fbce74";
  makeFlags = ''LLVM=1 W=1 \
      CC=${llvm.clang-unwrapped}/bin/clang LD=${llvm.lld}/bin/ld.lld \
      AR=${llvm.llvm}/bin/llvm-ar NM=${llvm.llvm}/bin/llvm-nm \
      STRIP=${llvm.llvm}/bin/llvm-strip OBJCOPY=${llvm.llvm}/bin/llvm-objcopy \
      OBJDUMP=${llvm.llvm}/bin/llvm-objdump READELF=${llvm.llvm}/bin/llvm-readelf \
      HOSTCC=${llvm.clang}/bin/clang HOSTCXX=${llvm.clang}/bin/clang++ \
      SHELL=${llvm.stdenv.shell}'';
in
llvm.stdenv.mkDerivation {
  pname = "fp6-kernel7-candidate-cs-call-audio-compile-check";
  version = "63289f441152-nixos6";
  inherit src;
  patches = [
    ./0005-ASoC-qcom-q6apm-dai-support-hostless-front-ends.patch
    ./0006-ASoC-qcom-audioreach-send-topology-module-config-for-any-module.patch
    ./0007-net-qrtr-forward-data-packets-between-endpoints.patch
  ];
  nativeBuildInputs = [ pkgs.flex pkgs.bison pkgs.perl pkgs.bc pkgs.openssl
                        llvm.lld llvm.llvm pkgs.kmod ];
  hardeningDisable = [ "pic" ];
  unpackPhase = ''
    runHook preUnpack
    mkdir work
    tar -xf "$src" -C work --strip-components=1 \
      ${top}/sound/soc/qcom/qdsp6 ${top}/sound/soc/qcom/common.h ${top}/net/qrtr ${top}/include/uapi/sound/snd_ar_tokens.h
    cd work
    runHook postUnpack
  '';
  postPatch = ''
    # q6apm.h includes "../common.h"
    mkdir -p m-q6apm/qdsp6 m-qrtr
    cp sound/soc/qcom/common.h m-q6apm/
    cp sound/soc/qcom/qdsp6/* m-q6apm/qdsp6/
    cat > m-q6apm/qdsp6/Makefile <<'MAKE'
    obj-m := snd-q6apm_fp6check.o q6apm-dai_fp6check.o
    snd-q6apm_fp6check-y := q6apm.o audioreach.o topology.o
    q6apm-dai_fp6check-y := q6apm-dai.o
    MAKE
    cp net/qrtr/* m-qrtr/
    cat > m-qrtr/Makefile <<'MAKE'
    obj-m := qrtr_fp6check.o
    qrtr_fp6check-y := af_qrtr.o ns.o
    MAKE
  '';
  buildPhase = ''
    runHook preBuild
    set -o pipefail
    make -j2 -C ${dev}/lib/modules/${release}/build M="$PWD/m-q6apm/qdsp6" ${makeFlags} modules 2>&1 | tee build-q6apm.log
    make -j2 -C ${dev}/lib/modules/${release}/build M="$PWD/m-qrtr" ${makeFlags} af_qrtr.o ns.o 2>&1 | tee build-qrtr.log
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    for ko in m-q6apm/qdsp6/snd-q6apm_fp6check.ko m-q6apm/qdsp6/q6apm-dai_fp6check.ko; do
      test "$(modinfo -F vermagic $ko | cut -d' ' -f1)" = ${release}
    done
    test -s m-qrtr/af_qrtr.o
    # The new hostless helper is exported and used by q6apm-dai.
    ${llvm.llvm}/bin/llvm-nm m-q6apm/qdsp6/snd-q6apm_fp6check.ko | grep -q ' __ksymtab_q6apm_is_graph_hostless\|q6apm_is_graph_hostless'
    ${llvm.llvm}/bin/llvm-nm m-q6apm/qdsp6/q6apm-dai_fp6check.ko | grep -q ' U q6apm_is_graph_hostless'
    # No warnings in the changed files.
    ! grep -nE '(q6apm-dai|q6apm|topology|audioreach|af_qrtr)\.c:[0-9]+:[0-9]+: warning' build-q6apm.log build-qrtr.log
  '';
  installPhase = ''
    d=$out/share/fp6-kernel7-candidate-cs-call-audio
    mkdir -p $d
    cp m-q6apm/qdsp6/*.ko build-q6apm.log build-qrtr.log $d/
    sha256sum sound/soc/qcom/qdsp6/{q6apm.c,q6apm-dai.c,topology.c,audioreach.c} net/qrtr/af_qrtr.c > $d/patched-source-sha256.txt
  '';
  dontStrip = true;
  dontFixup = true;
}
