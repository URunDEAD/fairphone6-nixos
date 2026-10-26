# fp6-batteryd (Rust): system daemon + session agent/indicator, GSettings
# schema com.fairphone6.battery and the Lomiri indicator file.
{ lib, rustPlatform, pkg-config, glib, dconf, makeWrapper, lomiri-url-dispatcher ? null
# Extra GSettings schema XMLs the agent may switch off as saver extras
# (e.g. com.fairphone6.gestures), compiled next to its own schema.
, extraSchemas ? [ ]
}:
rustPlatform.buildRustPackage {
  pname = "fp6-batteryd";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./fp6-batteryd;
    filter = path: _type: baseNameOf path != "target";
  };
  cargoLock.lockFile = ./fp6-batteryd/Cargo.lock;
  nativeBuildInputs = [ pkg-config glib makeWrapper ];
  buildInputs = [ glib ];
  # cargo test: charge-limit/adaptive policy, saver state machine, cpufreq
  # caps and threshold writes on fake sysfs, logger round trips, usage
  # accounting on fake cgroups, indicator alarm parsing.
  doCheck = true;
  postInstall = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    install -Dm644 ${./com.fairphone6.battery.gschema.xml} $schemas/com.fairphone6.battery.gschema.xml
    glib-compile-schemas --strict $schemas
    # The agent also needs the schemas of the saver extras (separate dir so
    # this package's own schema dir stays the one the Settings plugin uses).
    agent=$out/share/fp6-batteryd/agent-schemas
    install -Dm644 -t $agent ${./com.fairphone6.battery.gschema.xml} ${lib.concatMapStringsSep " " (p: "${p}") extraSchemas}
    glib-compile-schemas --strict $agent
    # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
    # module GSettings would fall back to the memory backend.
    wrapProgram $out/bin/fp6-batteryd \
      --set-default GSETTINGS_SCHEMA_DIR $agent \
      --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules \
      ${lib.optionalString (lomiri-url-dispatcher != null) "--prefix PATH : ${lomiri-url-dispatcher}/bin"}
    mkdir -p $out/share/ayatana/indicators
    cat > $out/share/ayatana/indicators/com.fairphone6.indicator.battery <<IND
    [Indicator Service]
    Name=fp6-battery-indicator
    ObjectPath=/com/fairphone6/indicator/battery
    Position=75

    [phone]
    ObjectPath=/com/fairphone6/indicator/battery/phone

    [phone_greeter]
    ObjectPath=/com/fairphone6/indicator/battery/phone_greeter

    [desktop]
    ObjectPath=/com/fairphone6/indicator/battery/desktop

    [desktop_greeter]
    ObjectPath=/com/fairphone6/indicator/battery/desktop_greeter
    IND
  '';
  passthru.schemaXml = ./com.fairphone6.battery.gschema.xml;
  meta = {
    description = "Battery and charging service for Lomiri on the Fairphone 6";
    license = lib.licenses.mit;
    mainProgram = "fp6-batteryd";
    platforms = lib.platforms.linux;
  };
}
