{ pkgs, vulkanSupport ? false, kleidiAiSupport ? false }:
let
  inherit (pkgs) lib;
  base = pkgs.llama-cpp.override {
    inherit vulkanSupport;
    cpuArchDynamicDispatch = !pkgs.stdenv.hostPlatform.isAarch64;
    cudaSupport = false;
    rocmSupport = false;
    metalSupport = false;
    blasSupport = false;
    openclSupport = false;
    rpcSupport = false;
  };
  # No FetchContent network access in the build sandbox. This is the exact
  # KleidiAI revision used by llama.cpp b9842; optional until benchmarked on FP6.
  kleidi = pkgs.fetchFromGitHub {
    owner = "ARM-software";
    repo = "kleidiai";
    tag = "v1.24.0";
    hash = "sha256-EqA/2Nu8/CGiBFK5EcCp6FVuBmmx6sV8x+g1Fx5HSh4=";
  };
in base.overrideAttrs (old: {
  pname = "fp6-llama-cpp" + lib.optionalString vulkanSupport "-vulkan";
  cmakeFlags = old.cmakeFlags ++ lib.optionals pkgs.stdenv.hostPlatform.isAarch64 [
    # Both A720 and A520 implement this ISA. Do not use -march=native on the builder.
    "-DGGML_CPU_ARM_ARCH=armv8.6-a+dotprod+i8mm+fp16"
    "-DGGML_CPU_KLEIDIAI=${if kleidiAiSupport then "ON" else "OFF"}"
  ] ++ lib.optionals (kleidiAiSupport && pkgs.stdenv.hostPlatform.isAarch64) [
    "-DFETCHCONTENT_SOURCE_DIR_KLEIDIAI_DOWNLOAD=${kleidi}"
  ];
})
