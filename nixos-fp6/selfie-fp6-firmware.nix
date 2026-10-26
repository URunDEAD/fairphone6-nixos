# Exact public FP6 vendor register-data extraction. No private/device input.
{ lib, stdenvNoCC, fetchurl, python3 }:
stdenvNoCC.mkDerivation {
  pname = "fp6-s5kkd1-firmware";
  version = "1";
  src = fetchurl {
    url = "https://raw.githubusercontent.com/TheMuppets/proprietary_vendor_fairphone_FP6/6c63b696c89bff29e4413b5290aff39626843b90/proprietary/vendor/lib64/camera/com.qti.sensormodule.tsp_s5kkd1sp_fp6.bin";
    sha256 = "7a924c8a79d526f0258896a0d048ac85b73fa989d273511b1e5e3e94f7f5d0ce";
  };
  dontUnpack = true;
  nativeBuildInputs = [ python3 ];
  buildPhase = ''
    python3 ${./selfie-fp6/extract-firmware.py} "$src" fp6-s5kkd1-v1.bin
  '';
  installPhase = ''
    install -Dm644 fp6-s5kkd1-v1.bin "$out/lib/firmware/fp6-s5kkd1-v1.bin"
    install -Dm644 ${./selfie-fp6/firmware-provenance.json} "$out/share/fp6-selfie/firmware-provenance.json"
    install -Dm644 ${./selfie-fp6/vendor-source-pins.json} "$out/share/fp6-selfie/vendor-source-pins.json"
  '';
  meta = {
    description = "Hash-pinned experimental FP6 S5KKD1 register-data payload";
    license = lib.licenses.unfree;
    platforms = [ "aarch64-linux" ];
  };
}
