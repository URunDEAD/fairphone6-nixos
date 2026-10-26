# fp6-app-ready daemon (Rust) + GSettings schema com.fairphone6.app-ready.
{ lib, rustPlatform, pkg-config, glib, dconf, makeWrapper, lomiri-app-launch }:
rustPlatform.buildRustPackage {
  pname = "fp6-app-ready";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-app-ready;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-app-ready/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  # cargo test: unit-name parsing, kill order, pressure levels, preload
  # learn/plan/fadvise/mincore on temp files, desktop entry filter.
  doCheck = true;
  postInstall = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.app-ready.gschema.xml} $schemas/com.fairphone6.app-ready.gschema.xml
    glib-compile-schemas --strict $schemas
    # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
    # module GSettings would fall back to the memory backend.
    wrapProgram $out/bin/fp6-app-ready \
      --set GSETTINGS_SCHEMA_DIR $schemas \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
      --set-default FP6_LOMIRI_APP_LAUNCH ${lomiri-app-launch}/bin/lomiri-app-launch
  '';
  passthru.schemaXml = ./com.fairphone6.app-ready.gschema.xml;
  meta = {
    description = "Keep chosen apps ready (frozen in memory, prelaunched, preloaded) for Lomiri on the Fairphone 6";
    license = lib.licenses.mit;
    mainProgram = "fp6-app-ready";
    platforms = lib.platforms.linux;
  };
}
