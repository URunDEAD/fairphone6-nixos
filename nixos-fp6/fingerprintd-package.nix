{ lib, llvmPackages_21, fetchgit, pkg-config, python3, glib }:
let
  qcomteeSource = fetchgit {
    url = "https://github.com/quic/quic-teec.git";
    rev = "736419e25a2036aac3292a10a93e394a90750ca3";
    hash = "sha256-HVtw2RT3LxwcRifzX9XT7nNsWPCJesVbuAhjPSgscus=";
  };
in llvmPackages_21.stdenv.mkDerivation {
  pname = "fingerprintd";
  version = "0.2.6";
  src = fetchgit {
    url = "https://forgejo.catcrafts.net/Catcrafts/fingerprintd.git";
    rev = "a4d5ed687749776fb0ef9cebe0de015bc2f8653b";
    hash = "sha256-S4tGr3DTIIkd+upxhdiDrJXPzhZWc3U4rE7mBpTJ0qM=";
  };
  nativeBuildInputs = [ pkg-config python3 ];
  buildInputs = [ glib ];
  QCOMTEE_SOURCE = qcomteeSource;
  dontConfigure = true;
  enableParallelBuilding = false;
  buildPhase = ''
    runHook preBuild
    python3 ${./fingerprintd-build.py}
    runHook postBuild
  '';
  # The build executes only upstream's pure state/wire-format tests and the
  # version early-return. No daemon, trustlet, GPIO, TEE or private state access.
  doCheck = false;
  installPhase = ''
    runHook preInstall
    install -Dm755 programs/fingerprintd "$out/bin/fingerprintd"
    install -Dm755 programs/fingerprintd-agent "$out/bin/fingerprintd-agent"
    install -Dm644 packaging/fingerprintd.json "$out/share/fingerprintd/fingerprintd.json"
    install -Dm644 packaging/net.reactivated.Fprint.conf "$out/share/dbus-1/system.d/net.reactivated.Fprint.conf"
    install -Dm644 packaging/net.reactivated.fprint.device.policy "$out/share/polkit-1/actions/net.reactivated.fprint.device.policy"
    # No upstream activation file, preset, PAM files, persist mount or agent
    # autostart: the separate opt-in module owns service configuration.
    runHook postInstall
  '';
  meta = {
    description = "FP6 FocalTech FT9391 userspace daemon over Qualcomm QTEE";
    homepage = "https://forgejo.catcrafts.net/Catcrafts/fingerprintd";
    license = lib.licenses.gpl3Only;
    platforms = [ "aarch64-linux" ];
    mainProgram = "fingerprintd";
  };
}
