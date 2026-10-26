# fp6-privacy (Rust): fp6-privacy-indicator, fp6-privacyd, fp6-privacy-system,
# plus the com.fairphone6.privacy schema (schema.nix) for the user services.
{ lib, rustPlatform, pkg-config, glib, dconf, makeWrapper, pipewire, networkmanager
, lomiri, fp6-privacy-schema }:
rustPlatform.buildRustPackage {
  pname = "fp6-privacy";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-privacy;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-privacy/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  # cargo test: pw-dump graph rules, SOS press counter and SMS text, Wi-Fi
  # keyfile migration, lock-screen JSON sanitising, /proc holder scan.
  doCheck = true;
  postInstall = ''
    # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
    # module GSettings would fall back to the memory backend.
    for b in fp6-privacy-indicator fp6-privacyd; do
      wrapProgram $out/bin/$b \
        --set GSETTINGS_SCHEMA_DIR ${fp6-privacy-schema}/share/gsettings-schemas/${fp6-privacy-schema.name}/glib-2.0/schemas \
        --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
        --prefix PATH : ${lib.makeBinPath [ pipewire networkmanager lomiri.lomiri-url-dispatcher lomiri.lomiri-app-launch ]}
    done
  '';
  meta = {
    description = "FP6 Lomiri privacy & lock helpers (in-use indicator, camera monitor, SOS, emergency info, Wi-Fi MAC policy)";
    license = lib.licenses.gpl3Only;
    mainProgram = "fp6-privacyd";
    platforms = lib.platforms.linux;
  };
}
