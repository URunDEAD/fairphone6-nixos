# fp6-display-comfort daemon (Rust) + GSettings schema com.fairphone6.display-comfort.
{ lib, rustPlatform, pkg-config, glib, dconf, makeWrapper, lomiri }:
rustPlatform.buildRustPackage {
  pname = "fp6-display-comfort";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-display-comfort;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-display-comfort/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  # cargo test: NOAA sunrise/sunset (London, Bucharest, polar), schedules and
  # ramps, mired interpolation, ini edits, session.env.
  doCheck = true;
  postInstall = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.display-comfort.gschema.xml} $schemas/com.fairphone6.display-comfort.gschema.xml
    glib-compile-schemas --strict $schemas
    # User units do not get the session's GIO_EXTRA_MODULES / XDG_DATA_DIRS.
    # com.lomiri.Shell (light-mode) comes from lomiri-schemas.
    wrapProgram $out/bin/fp6-display-comfort \
      --prefix XDG_DATA_DIRS : $out/share/gsettings-schemas/$name:${lomiri.lomiri-schemas}/share/gsettings-schemas/${lomiri.lomiri-schemas.name} \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
      --prefix PATH : ${lib.getBin dconf}/bin
  '';
  passthru.schemaXml = ./com.fairphone6.display-comfort.gschema.xml;
  meta = {
    description = "Night light / dark theme schedule and scale settings for Lomiri on the Fairphone 6";
    license = lib.licenses.mit;
    mainProgram = "fp6-display-comfort";
    platforms = lib.platforms.linux;
  };
}
