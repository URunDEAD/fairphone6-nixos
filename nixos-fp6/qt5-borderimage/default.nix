let
  src = builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  };
  pkgs = import src { system = "aarch64-linux"; config = {}; overlays = []; };
in pkgs.libsForQt5.qtdeclarative.overrideAttrs (old: {
  patches = (old.patches or []) ++ [ ./stale-reply.patch ];
  postFixup = (old.postFixup or "") + ''
    env QT_PLUGIN_PATH=${pkgs.libsForQt5.qtbase.bin}/lib/qt-5.15.19/plugins \
      QML2_IMPORT_PATH=$bin/lib/qt-5.15.19/qml \
      QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software \
      timeout 10s $dev/bin/qmlscene ${./stale-reply.qml}
  '';
})
