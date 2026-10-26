# biometryd device plugin backed by fprintd (fingerprintd on the FP6).
# Built against the exact biometryd the Lomiri set uses (pkgs.lomiri.biometryd),
# so the plugin's descriptor carries that host version (1.0.1) and the loader's
# major-version check passes.
{ lib, stdenv, biometryd, boost, systemd, pkg-config }:
stdenv.mkDerivation {
  pname = "fp6-biometryd-fprintd";
  version = "0.1.0";
  src = ./.;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ biometryd biometryd.dev boost systemd ];
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    $CXX -std=c++17 -O2 -g -Wall -Wextra -Wno-unused-parameter -fPIC -shared \
      -fvisibility=hidden \
      -I${biometryd.dev}/include \
      $(pkg-config --cflags libsystemd) \
      fp6_biometryd_fprintd.cpp -o libfp6-biometryd-fprintd.so \
      -L${biometryd}/lib -lbiometry $(pkg-config --libs libsystemd) -lpthread \
      -Wl,-rpath,${biometryd}/lib -Wl,--no-undefined
    # Test/CLI client over biometryd's own D-Bus stub.
    $CXX -std=c++17 -O2 -Wall -Wextra -Wno-unused-parameter \
      -I${biometryd.dev}/include fp6_biometryd_probe.cpp -o fp6-biometryd-probe \
      -L${biometryd}/lib -lbiometry -lpthread -Wl,-rpath,${biometryd}/lib
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 libfp6-biometryd-fprintd.so $out/lib/biometryd/plugins/libfp6-biometryd-fprintd.so
    install -Dm755 fp6-biometryd-probe $out/bin/fp6-biometryd-probe
    runHook postInstall
  '';
  # The plugin is found by biometryd through this ELF section; fail the build
  # if the descriptor or the entry points went missing.
  doInstallCheck = true;
  installCheckPhase = ''
    so=$out/lib/biometryd/plugins/libfp6-biometryd-fprintd.so
    ${stdenv.cc.targetPrefix}readelf -S -W "$so" | grep -q BIOMETRYD_DEVICES_PLUGIN_DESCRIPTOR
    ${stdenv.cc.targetPrefix}nm -D --defined-only "$so" | grep -q ' CreateBiometrydDevice$'
    ${stdenv.cc.targetPrefix}nm -D --defined-only "$so" | grep -q ' DestroyBiometrydDevice$'
  '';
  meta = {
    description = "biometryd device plugin bridging to fprintd (net.reactivated.Fprint)";
    license = lib.licenses.lgpl3Only;
    platforms = lib.platforms.linux;
  };
}
