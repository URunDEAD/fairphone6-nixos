# Persistence module packages (see persistence-fp6.nix).
#  schema:     com.fairphone6.vpn alone (the Settings plugin and the agent).
#  vpnRestore: fp6-vpn-restore (Rust, gio): reconnects NM VPNs after a restart.
#  lssPlugin:  Fp6VpnStartup.qml for Settings > VPN (merged into the stock
#              vpn plugin dir by the lomiri-system-settings wrapper; the
#              page itself is patched in the wrapper's postInstall).
{ lib, stdenvNoCC, rustPlatform, pkg-config, glib, dconf, makeWrapper }:
let
  schema = stdenvNoCC.mkDerivation {
    name = "fp6-vpn-schema-1";
    dontUnpack = true;
    nativeBuildInputs = [ glib ];
    installPhase = ''
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.vpn.gschema.xml} $schemas/com.fairphone6.vpn.gschema.xml
      glib-compile-schemas --strict $schemas
    '';
  };

  vpnRestore = rustPlatform.buildRustPackage {
    pname = "fp6-vpn-restore";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./fp6-vpn-restore;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./fp6-vpn-restore/Cargo.lock;
    nativeBuildInputs = [ pkg-config glib makeWrapper ];
    buildInputs = [ glib ];
    # cargo test: the startup policy (restore/always/never, user intent vs.
    # drops, back-off and give-up, profile removal, NM start races).
    doCheck = true;
    postInstall = ''
      # User units do not get the session's GIO_EXTRA_MODULES: without dconf's
      # module GSettings would fall back to the memory backend.
      wrapProgram $out/bin/fp6-vpn-restore \
        --set-default GSETTINGS_SCHEMA_DIR ${schema}/share/gsettings-schemas/${schema.name}/glib-2.0/schemas \
        --prefix GIO_EXTRA_MODULES : ${lib.getLib dconf}/lib/gio/modules
    '';
    meta = {
      description = "Reconnects NetworkManager VPNs after a restart (Lomiri, Fairphone 6)";
      license = lib.licenses.mit;
      mainProgram = "fp6-vpn-restore";
      platforms = lib.platforms.linux;
    };
  };

  lssPlugin = stdenvNoCC.mkDerivation {
    pname = "lomiri-system-settings-fp6-vpn-startup";
    version = "1";
    src = ./settings-page;
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/vpn Fp6VpnStartup.qml
      # The wrapper lndir's these from every plugin.
      mkdir -p $out/lib/lomiri-system-settings $out/share/locale $out/share/gsettings-schemas
      ln -s ${schema}/share/gsettings-schemas/${schema.name} $out/share/gsettings-schemas/
      runHook postInstall
    '';
    meta = {
      description = "Reconnect-at-startup section for the Lomiri System Settings VPN page";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit schema vpnRestore lssPlugin; }
