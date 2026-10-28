# FP6 Assistant: fp6-assistantd (agent, model backends, tools, permissions;
# D-Bus com.fairphone6.Assistant1), fp6-assistant (Lomiri UITK 1.3 app,
# qmetaobject bridge) and fp6-assistant-ctl (CLI).
{ lib, rustPlatform, pkg-config, sqlite, dbus, makeWrapper, qt5, libsForQt5, lomiri, qtorganizer-mkcal, librsvg }:
rustPlatform.buildRustPackage {
  pname = "fp6-assistant";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./rust/Cargo.toml ./rust/Cargo.lock ./rust/src ./rust/tests ./qml ./data ];
  };
  sourceRoot = "source/rust";
  cargoLock.lockFile = ./rust/Cargo.lock;
  buildFeatures = [ "ui" ];
  nativeBuildInputs = [ pkg-config qt5.qmake qt5.wrapQtAppsHook makeWrapper librsvg ];
  buildInputs = [ sqlite qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtsvg libsForQt5.qtpim
    lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub qtorganizer-mkcal ];
  # cargo test: SSE parsing, OpenAI/Anthropic request/stream shapes, the
  # agent loop end to end on a private dbus-daemon with fake history /
  # telephony / indicator / on-device-LLM services and a fake model server
  # (contacts -> SMS with confirmation, prompt injection blocked, denial
  # is final, rate limit, stop, local-only), D-Bus caller restrictions,
  # file scopes and symlinks, keyring file fallback, manifests.
  nativeCheckInputs = [ dbus ];
  doCheck = true;
  postInstall = ''
    share=$out/share/fp6-assistant
    mkdir -p $share/qml $out/share/applications $out/share/lomiri-url-dispatcher/urls $out/share/dbus-1/services \
      $out/share/icons/hicolor/scalable/apps $out/share/icons/hicolor/256x256/apps
    cp ../qml/*.qml $share/qml/
    install -m644 ../data/fp6-assistant.svg $out/share/icons/hicolor/scalable/apps/fp6-assistant.svg
    rsvg-convert -w 256 -h 256 ../data/fp6-assistant.svg -o $out/share/icons/hicolor/256x256/apps/fp6-assistant.png
    # lomiri-app-launch wants an absolute icon path for non-click apps.
    sed -e "s|@ICON@|$out/share/icons/hicolor/256x256/apps/fp6-assistant.png|" \
        -e "s|^Exec=fp6-assistant|Exec=$out/bin/fp6-assistant|" \
        ../data/com.fairphone6.Assistant.desktop > $out/share/applications/com.fairphone6.Assistant.desktop
    install -m644 ../data/com.fairphone6.Assistant.url-dispatcher $out/share/lomiri-url-dispatcher/urls/
    substitute ../data/com.fairphone6.Assistant1.service $out/share/dbus-1/services/com.fairphone6.Assistant1.service --subst-var out
    qtWrapperArgs+=(
      --set-default APP_ID "com.fairphone6.Assistant"
      --set FP6_ASSISTANT_QML_DIR "$share/qml"
      # Alarms in mkcal, where the calendar app and indicator-datetime look
      # (lomiri-ui-toolkit defaults to "eds", which this system does not have).
      --set-default ALARM_BACKEND mkcal
      --prefix QT_PLUGIN_PATH : "${qtorganizer-mkcal}/${qt5.qtbase.qtPluginPrefix}:${libsForQt5.qtpim}/${qt5.qtbase.qtPluginPrefix}"
      --prefix XDG_DATA_DIRS : "${lomiri.suru-icon-theme}/share"
      --prefix QML2_IMPORT_PATH : "${lomiri.lomiri-ui-toolkit}/${qt5.qtbase.qtQmlPrefix}:${lomiri.lomiri-content-hub}/${qt5.qtbase.qtQmlPrefix}:${libsForQt5.qtpim}/${qt5.qtbase.qtQmlPrefix}"
    )
  '';
  # Only the UI is a Qt program; the daemon and the CLI stay unwrapped
  # (the daemon trusts callers from this bin/ directory: the wrapped UI
  # runs as bin/.fp6-assistant-wrapped).
  dontWrapQtApps = true;
  postFixup = ''
    wrapQtApp $out/bin/fp6-assistant
  '';
  meta = {
    description = "On-device assistant for the Fairphone 6: chat with a local or remote model that uses the phone's apps through permission-gated tools";
    license = lib.licenses.gpl3Plus;
    mainProgram = "fp6-assistant";
    platforms = lib.platforms.linux;
  };
}
