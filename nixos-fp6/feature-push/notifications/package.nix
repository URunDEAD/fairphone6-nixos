# fp6-notifyd (Rust): DND/bedtime state, com.fairphone6.Dnd, status-bar
# indicator, notification history observer.
{ lib, rustPlatform, pkg-config, glib, dconf, makeWrapper
, greyscaleKey ? "" }:
rustPlatform.buildRustPackage {
  pname = "fp6-notifyd";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./fp6-notifyd;
    fileset = lib.fileset.unions [ ./fp6-notifyd/Cargo.toml ./fp6-notifyd/Cargo.lock ./fp6-notifyd/src ];
  };
  cargoLock.lockFile = ./fp6-notifyd/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  # cargo test: schedule windows (overnight, weekdays), manual override
  # expiry, bedtime, call/message exceptions (favourites, repeat callers,
  # allowed numbers), history store (off by default, body stripping,
  # retention, 0600, clear).
  doCheck = true;
  postInstall = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.notifications.gschema.xml} $schemas/com.fairphone6.notifications.gschema.xml
    glib-compile-schemas --strict $schemas
    # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
    # module GSettings would fall back to the memory backend.
    wrapProgram $out/bin/fp6-notifyd \
      --set GSETTINGS_SCHEMA_DIR $schemas \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
      --set-default FP6_DCONF ${lib.getBin dconf}/bin/dconf \
      --set-default FP6_GREYSCALE_KEY '${greyscaleKey}'
  '';
  meta = {
    description = "Do Not Disturb, bedtime mode and notification history for Lomiri on the Fairphone 6";
    license = lib.licenses.gpl3Only;
    mainProgram = "fp6-notifyd";
    platforms = lib.platforms.linux;
  };
}
