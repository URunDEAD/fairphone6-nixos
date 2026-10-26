# Only extracts headers + AArch64 target runtimes. No NDK x86 tool executes.
{ stdenvNoCC, fetchurl, python3 }:
stdenvNoCC.mkDerivation {
  pname = "fp6-android-ndk-data";
  version = "r27d";
  src = fetchurl {
    url = "https://dl.google.com/android/repository/android-ndk-r27d-linux.zip";
    hash = "sha256-YBJGCHpoLRlE4eFt2FvG5JVg/ottYSVb4oKReMjtFdk=";
  };
  nativeBuildInputs = [ python3 ];
  dontUnpack = true;
  dontFixup = true;
  installPhase = ''
    mkdir -p $out
    python3 ${./build-support/extract-ndk-sysroot.py} \
      --archive "$src" --destination "$out"
  '';
}
