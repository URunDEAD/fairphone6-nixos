# fp6-crash-notify (Rust): "<App> stopped" / shell-restarted notifications.
{ lib, rustPlatform, pkg-config, glib }:
rustPlatform.buildRustPackage {
  pname = "fp6-crash-notify";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-crash-notify;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-crash-notify/Cargo.lock;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ glib ];
  # cargo test: unit path decoding, app-id parsing, crash-signal filter
  # (SIGKILL/SIGTERM/exit codes are not crashes), desktop entries, rate limit.
  doCheck = true;
  meta = {
    description = "Crash notifications with a Report action for Lomiri on the Fairphone 6";
    license = lib.licenses.mit;
    mainProgram = "fp6-crash-notify";
    platforms = lib.platforms.linux;
  };
}
