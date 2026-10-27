# fp6-accessibility-audio: GSettings (mono-audio, audio-balance) ->
# WirePlumber settings bridge (Rust). cargo test runs the value/argument tests.
{ lib, rustPlatform, pkg-config, glib, dconf, pipewire, makeWrapper, schema }:
rustPlatform.buildRustPackage {
  pname = "fp6-accessibility-audio";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: _type: !(builtins.elem (baseNameOf path) [ "target" "package.nix" ]);
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  doCheck = true;
  # User units do not get the session's GIO_EXTRA_MODULES / XDG_DATA_DIRS.
  postInstall = ''
    wrapProgram $out/bin/fp6-accessibility-audio \
      --prefix XDG_DATA_DIRS : ${schema}/share/gsettings-schemas/${schema.name} \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
      --prefix PATH : ${lib.getBin pipewire}/bin
  '';
  meta = {
    description = "Mono audio / balance bridge (GSettings -> WirePlumber) for the Fairphone 6";
    license = lib.licenses.mit;
    mainProgram = "fp6-accessibility-audio";
    platforms = lib.platforms.linux;
  };
}
