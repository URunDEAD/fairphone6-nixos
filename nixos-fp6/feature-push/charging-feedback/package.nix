# FP6 charging feedback: fp6-charging-feedback (Rust session agent: plug
# detection, speed, time to full), the com.fairphone6.charging-feedback
# schema (Lomiri + System Settings) and the Battery > Charging feedback page.
{ lib, stdenvNoCC, rustPlatform, pkg-config, glib }:
let
  core = rustPlatform.buildRustPackage {
    pname = "fp6-charging-feedback";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    nativeBuildInputs = [ pkg-config glib ];
    buildInputs = [ glib ];
    # cargo test: plug/unplug debouncing (bounce, power-role swap), speed
    # classes (3 W port, PD, DCP, taper), status/limit/adaptive, time to
    # full sources, fp6-batteryd/fp6-usbmoded status parsing, a full plug
    # cycle on a fake FP6 power_supply tree.
    doCheck = true;
    meta = {
      description = "Charger plug detection and charging state for the Lomiri charging animation on the Fairphone 6";
      license = lib.licenses.gpl3Plus;
      mainProgram = "fp6-charging-feedback";
      platforms = lib.platforms.linux;
    };
  };
  schemaXml = ./com.fairphone6.charging-feedback.gschema.xml;
  installSchema = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${schemaXml} $schemas/com.fairphone6.charging-feedback.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
  schema = stdenvNoCC.mkDerivation {
    pname = "fp6-charging-feedback-schema";
    version = "0.1.0";
    dontUnpack = true;
    nativeBuildInputs = [ glib ];
    installPhase = installSchema;
  };
  lssPlugin = stdenvNoCC.mkDerivation {
    pname = "lomiri-system-settings-charging-feedback";
    version = "0.1.0";
    src = ./settings-page;
    nativeBuildInputs = [ glib ];
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      # Into the stock battery plugin's directory (the wrapper lndir's every
      # plugin into one tree); Fp6BatteryExtras.qml links to it.
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/battery Fp6ChargingFeedbackPage.qml
      mkdir -p $out/lib/lomiri-system-settings $out/share/locale
      ${installSchema}
      runHook postInstall
    '';
    meta = {
      description = "Battery > Charging feedback page (animation, sound, vibration, lock screen) for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit core schema schemaXml lssPlugin; }
