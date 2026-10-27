# fp6-csvoiced (Rust): starts/stops the hostless VoiceMMode1 PCM while
# oFono reports a CS call that needs audio.
{ lib, rustPlatform, pkg-config, alsa-lib }:
rustPlatform.buildRustPackage {
  pname = "fp6-csvoiced";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-csvoiced;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-csvoiced/Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ alsa-lib ];
  # cargo test: config parser, call-state controller (start/stop/retry,
  # mute/volume writes), /proc/asound parsers.
  doCheck = true;
  meta = {
    description = "CS call voice-path daemon for the Fairphone 6 (oFono -> q6apm VoiceMMode1)";
    license = lib.licenses.mit;
    mainProgram = "fp6-csvoiced";
    platforms = lib.platforms.linux;
  };
}
