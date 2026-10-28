# FP6 app permissions: fp6-permd (system broker), fp6-permission-agent
# (session agent: prompts, focus, Settings data), fp6-permctl (CLI), the
# Lomiri prompt QML and the System Settings pages.
{ lib, stdenvNoCC, rustPlatform, pkg-config, glib }:
let
  core = rustPlatform.buildRustPackage {
    pname = "fp6-permissions";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    nativeBuildInputs = [ pkg-config glib ];
    buildInputs = [ glib ];
    # cargo test: unit-name/cgroup/AppArmor-label identity, decisions and
    # "while using", device node rules, store/history, enforcement renderers,
    # broker logic on a scratch tree (tests/broker.rs).
    doCheck = true;
    postInstall = ''
      # Largest AppArmor includes, for the profile compile check (system.checks).
      mkdir -p $out/share/fp6-permissions
      $out/bin/fp6-permd --sample-include allow > $out/share/fp6-permissions/sample-allow.inc
      $out/bin/fp6-permd --sample-include deny > $out/share/fp6-permissions/sample-deny.inc
    '';
    meta = {
      description = "Runtime app permissions (USB, NFC, Bluetooth, contacts, calendar, messages, files, sensors, network) for Lomiri on the Fairphone 6";
      license = lib.licenses.gpl3Plus;
      mainProgram = "fp6-permd";
      platforms = lib.platforms.linux;
    };
  };
  lssPlugin = stdenvNoCC.mkDerivation {
    pname = "lomiri-system-settings-fp6-app-permissions";
    version = "0.1.0";
    src = ./settings-page;
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      # Security & Privacy > App permissions (AppAccess.qml) imports
      # "../fp6-app-permissions" (lss-app-access.patch).
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/fp6-app-permissions *.qml
      mkdir -p $out/lib/lomiri-system-settings $out/share/locale
      runHook postInstall
    '';
    meta = {
      description = "Security & Privacy > App permissions pages (per permission, per app, network, history) for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit core lssPlugin; }
