# fp6-gestures daemon (Rust) + GSettings schema com.fairphone6.gestures.
{ lib, rustPlatform, pkg-config, glib, dconf, makeWrapper }:
rustPlatform.buildRustPackage {
  pname = "fp6-gestures";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-gestures;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-gestures/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  # cargo test: detector on the recorded rest trace + synthetic motions,
  # lock policy, app list filter, command timeout (uses /bin/sh).
  doCheck = true;
  postInstall = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.gestures.gschema.xml} $schemas/com.fairphone6.gestures.gschema.xml
    glib-compile-schemas --strict $schemas
    # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
    # module GSettings would fall back to the memory backend.
    wrapProgram $out/bin/fp6-gestures \
      --set GSETTINGS_SCHEMA_DIR $schemas \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules
  '';
  passthru.schemaXml = ./com.fairphone6.gestures.gschema.xml;
  meta = {
    description = "Shake and power-key multi-press shortcuts for Lomiri on the Fairphone 6";
    license = lib.licenses.mit;
    mainProgram = "fp6-gestures";
    platforms = lib.platforms.linux;
  };
}
