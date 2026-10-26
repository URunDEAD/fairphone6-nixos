{ lib, stdenv, fetchFromGitHub, systemd }:
stdenv.mkDerivation {
  pname = "umtprd";
  version = "1.8.1";
  src = fetchFromGitHub {
    owner = "viveris";
    repo = "uMTP-Responder";
    rev = "8f87f7b19034055e2665eaaf07f84a41aa5f25e0";
    sha256 = "0lwcm66zf1avsk17zvxb4s1b5plfjn9k5s4qp6dp4d5i1h9fx5ci";
  };
  buildInputs = [ systemd ];
  makeFlags = [ "CC=${stdenv.cc.targetPrefix}cc" "SYSTEMD=1" "USE_SYSLOG=1" ];
  enableParallelBuilding = true;
  installPhase = ''
    runHook preInstall
    install -Dm755 umtprd $out/bin/umtprd
    install -Dm644 conf/umtprd.conf $out/share/umtprd/example.conf
    runHook postInstall
  '';
  meta = {
    description = "USB MTP responder with FunctionFS and systemd readiness support";
    homepage = "https://github.com/viveris/uMTP-Responder";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "umtprd";
  };
}
