# FP6 USB modes: Rust crate (fp6-usbmoded root helper, fp6-usbmodes-agent
# session agent/indicator, fp6-usbmodes-cli), the GSettings schema
# com.fairphone6.usb-modes and the System Settings > USB page.
{ lib, stdenvNoCC, rustPlatform, pkg-config, glib, dconf, makeWrapper, lomiri-url-dispatcher ? null }:
let
  core = rustPlatform.buildRustPackage {
    pname = "fp6-usb-modes";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    nativeBuildInputs = [ pkg-config glib makeWrapper ];
    buildInputs = [ glib ];
    # cargo test: typec/power_supply parsing and power-role writes on fake
    # sysfs, NCM link changes with rollback on a fake configfs, the
    # connection policy (boot/plug/unplug/unlock/ask/per-host), plan +
    # rollback of function-set transitions, and the helper engine
    # (plug settling, unplug debounce, charge-only, exclusive USB drive).
    doCheck = true;
    postInstall = ''
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.usb-modes.gschema.xml} $schemas/com.fairphone6.usb-modes.gschema.xml
      glib-compile-schemas --strict $schemas
      # User units do not get the session's GIO_EXTRA_MODULES (dconf).
      wrapProgram $out/bin/fp6-usbmodes-agent \
        --set-default GSETTINGS_SCHEMA_DIR $schemas \
        --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
        ${lib.optionalString (lomiri-url-dispatcher != null) "--prefix PATH : ${lomiri-url-dispatcher}/bin"}
      mkdir -p $out/share/ayatana/indicators
      cat > $out/share/ayatana/indicators/com.fairphone6.indicator.usbmodes <<IND
      [Indicator Service]
      Name=fp6-usb-modes-indicator
      ObjectPath=/com/fairphone6/indicator/usbmodes
      Position=71

      [phone]
      ObjectPath=/com/fairphone6/indicator/usbmodes/phone

      [phone_greeter]
      ObjectPath=/com/fairphone6/indicator/usbmodes/phone_greeter

      [desktop]
      ObjectPath=/com/fairphone6/indicator/usbmodes/desktop

      [desktop_greeter]
      ObjectPath=/com/fairphone6/indicator/usbmodes/desktop_greeter
      IND
    '';
    meta = {
      description = "USB connection preferences (functions and power role) for Lomiri on the Fairphone 6";
      license = lib.licenses.gpl3Plus;
      mainProgram = "fp6-usbmoded";
    };
  };
  lssPlugin = stdenvNoCC.mkDerivation {
    pname = "lomiri-system-settings-usb-modes";
    version = "0.1.0";
    src = ./settings-page;
    nativeBuildInputs = [ glib ];
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      install -Dm644 usb-modes.settings $out/share/lomiri-system-settings/usb-modes.settings
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/usb-modes PageComponent.qml
      # The wrapper lndir's these from every plugin.
      mkdir -p $out/lib/lomiri-system-settings $out/share/locale
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.usb-modes.gschema.xml} $schemas/com.fairphone6.usb-modes.gschema.xml
      glib-compile-schemas --strict $schemas
      runHook postInstall
    '';
    meta = {
      description = "USB page (connection preferences) for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit core lssPlugin; }
