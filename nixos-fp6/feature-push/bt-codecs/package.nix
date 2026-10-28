{ lib, rustPlatform, pkg-config, libpulseaudio, dbus }:
rustPlatform.buildRustPackage {
  pname = "fp6-bluetooth-audio";
  version = "0.1.0";
  src = lib.fileset.toSource { root = ./.; fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./tests/service.rs ./scripts/test.sh ]; };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ libpulseaudio ];
  nativeCheckInputs = [ dbus ];
  checkPhase = ''runHook preCheck
    bash ./scripts/test.sh
    runHook postCheck'';
  meta = { description = "Per-device Bluetooth audio controls for PipeWire"; license = lib.licenses.mit; platforms = lib.platforms.linux; mainProgram = "fp6-bluetooth-audio"; };
}
