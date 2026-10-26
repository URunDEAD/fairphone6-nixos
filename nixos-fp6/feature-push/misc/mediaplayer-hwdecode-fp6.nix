{ config, lib, pkgs, ... }:
# lomiri-mediaplayer-app with Iris (qcom-iris-decoder, stateful V4L2 M2M)
# hardware decode.
#
# Iris is a STATEFUL decoder: GStreamer's matching elements are
# v4l2h264dec / v4l2h265dec / v4l2vp9dec from gst-plugins-good "video4linux2"
# (verified on the phone 2026-10-24 with gst-inspect), not the stateless
# v4l2codecs (v4l2slh264dec) from -bad. The nixpkgs mediaplayer wrapper
# already carries gst-plugins-good; v4l2 decoders register at PRIMARY+1, so
# playbin normally prefers them. This wrapper makes that explicit and keeps
# the presentation path that passed on hardware (results/qt-multimedia-
# playback): NV12 NoHandle CPU mapping -> Qt YUV scenegraph node.
#
# zeroCopy = true is the DMA-BUF -> GL experiment (Qt GStreamer GL plugin,
# glupload of decoder dmabufs). It requires an accepted Adreno GL session
# (see GPU-and-Rendering.md); default OFF.
let
  cfg = config.services.fp6MediaplayerHwDecode;
  orig = pkgs.lomiri.lomiri-mediaplayer-app;
  ranks = lib.concatMapStringsSep "," (e: "${e}:MAX") [ "v4l2h264dec" "v4l2h265dec" "v4l2vp9dec" ];
  wrapped = pkgs.symlinkJoin {
    name = "lomiri-mediaplayer-app-fp6-hwdecode";
    paths = [ orig ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      rm "$out/bin/lomiri-mediaplayer-app"
      makeWrapper ${orig}/bin/lomiri-mediaplayer-app "$out/bin/lomiri-mediaplayer-app" \
        --set GST_PLUGIN_FEATURE_RANK ${lib.escapeShellArg ranks} \
        --set QT_GSTREAMER_USE_OPENGL_PLUGIN ${if cfg.zeroCopy then "1" else "0"} \
        ${lib.optionalString (!cfg.zeroCopy) "--set QT_QUICK_NO_TEXTURE_VIDEOFRAMES 1"} \
        ${lib.optionalString cfg.debug "--set GST_DEBUG 'v4l2*:4,decodebin*:3,playbin*:3'"}
      # .desktop uses Exec=lomiri-mediaplayer-app (PATH lookup); point it at us explicitly.
      if [ -L "$out/share/applications/lomiri-mediaplayer-app.desktop" ]; then
        src=$(readlink -f "$out/share/applications/lomiri-mediaplayer-app.desktop")
        rm "$out/share/applications/lomiri-mediaplayer-app.desktop"
        sed "s|^Exec=lomiri-mediaplayer-app|Exec=$out/bin/lomiri-mediaplayer-app|" "$src" \
          > "$out/share/applications/lomiri-mediaplayer-app.desktop"
      fi
    '';
  };
in {
  options.services.fp6MediaplayerHwDecode = {
    enable = lib.mkEnableOption "lomiri-mediaplayer-app with FP6 Iris V4L2 hardware decode";
    zeroCopy = lib.mkOption { type = lib.types.bool; default = false; description = "EXPERIMENTAL DMA-BUF->GL path; needs accepted GPU session."; };
    debug = lib.mkOption { type = lib.types.bool; default = false; description = "Log v4l2/decodebin element selection to the journal."; };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ (lib.hiPrio wrapped) pkgs.gst_all_1.gstreamer.bin ];
    system.build.fp6-mediaplayer-hwdecode = wrapped;
    # Iris nodes are root:video 0660; the phone user is already in "video".
    assertions = [{
      assertion = builtins.elem "video" (config.users.users.user.extraGroups or [ ]);
      message = "fp6MediaplayerHwDecode: phone user must be in the video group for /dev/video* (Iris)";
    }];
  };
}
