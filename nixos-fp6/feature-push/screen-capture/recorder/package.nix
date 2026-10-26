# fp6-screenrecord (encoder Lomiri spawns) + fp6-screenrecord-indicator.
{ lib, rustPlatform, pkg-config, makeWrapper, glib, gst_all_1 }:
let
  gstPlugins = with gst_all_1; [
    gstreamer.out
    gst-plugins-base   # appsrc, videoconvert, videorate, audioconvert/resample, audiomixer
    gst-plugins-good   # v4l2h264enc, videoflip, mp4mux, pulsesrc
    gst-plugins-bad    # voaacenc, h264parse
    gst-plugins-ugly   # x264enc (fallback)
  ];
in rustPlatform.buildRustPackage {
  pname = "fp6-screen-capture";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config makeWrapper ];
  buildInputs = [ glib gst_all_1.gstreamer gst_all_1.gst-plugins-base ];
  postInstall = ''
    wrapProgram $out/bin/fp6-screenrecord \
      --set GST_PLUGIN_SYSTEM_PATH_1_0 ${lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins} \
      --run 'export GST_REGISTRY_1_0="''${XDG_RUNTIME_DIR:-/tmp}/fp6-screenrecord-gst-registry.bin"'
  '';
  passthru = { inherit gstPlugins; };
  meta = {
    description = "FP6 Lomiri screen recorder: encoder and status-bar indicator";
    license = lib.licenses.gpl3Only;
    mainProgram = "fp6-screenrecord";
  };
}
