# fp6-callrec (Rust): the call recorder user service and CLI, plus the
# "Call recording" desktop entry the privacy indicator names it by.
{ lib, rustPlatform, pkg-config, glib, sqlite, alsa-lib, dconf, makeWrapper
, pipewire, flac, opus-tools, lomiri, fp6-callrecording-schema }:
rustPlatform.buildRustPackage {
  pname = "fp6-callrec";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-callrec;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-callrec/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib sqlite alsa-lib ];
  # cargo test: call-state machine, naming/sanitising, sidecar, timeline
  # (lead-in/dropouts), WAV writer, pw-dump graph and link choice, oFono/imsd
  # mapping, contacts lookup, CS config, D-Bus action descriptions.
  doCheck = true;
  postInstall = ''
    # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
    # module GSettings would fall back to the memory backend.
    wrapProgram $out/bin/fp6-callrec \
      --set GSETTINGS_SCHEMA_DIR ${fp6-callrecording-schema}/share/gsettings-schemas/${fp6-callrecording-schema.name}/glib-2.0/schemas \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
      --prefix PATH : ${lib.makeBinPath [ pipewire flac opus-tools ]}
    # fp6-privacy-indicator names microphone users by APP_ID -> desktop Name.
    install -Dm644 /dev/stdin $out/share/applications/fp6-callrec.desktop <<DESKTOP
    [Desktop Entry]
    Type=Application
    Name=Call recording
    Comment=Phone call recorder (Phone app > Settings > Call recording)
    Icon=media-record
    Exec=${lomiri.lomiri-url-dispatcher}/bin/lomiri-url-dispatcher application:///lomiri-dialer-app.desktop
    NoDisplay=true
    DESKTOP
  '';
  meta = {
    description = "FP6 call recorder: oFono/imsd call state -> separate uplink/downlink files with a sidecar";
    license = lib.licenses.gpl3Only;
    mainProgram = "fp6-callrec";
    platforms = lib.platforms.linux;
  };
}
