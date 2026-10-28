# fp6-notify-reply (Rust): test client for inline replies; sends a synthetic
# notification with the "inline-reply" action and prints the reply.
{ lib, rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "fp6-notify-reply";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./fp6-notify-reply;
    fileset = lib.fileset.unions [ ./fp6-notify-reply/Cargo.toml ./fp6-notify-reply/Cargo.lock ./fp6-notify-reply/src ];
  };
  cargoLock.lockFile = ./fp6-notify-reply/Cargo.lock;
  # cargo test: argument parsing, reports (text/JSON) and exit codes.
  doCheck = true;
  meta = {
    description = "Sends a notification with an inline reply and prints the reply (FP6 test client)";
    license = lib.licenses.gpl3Only;
    mainProgram = "fp6-notify-reply";
    platforms = lib.platforms.linux;
  };
}
