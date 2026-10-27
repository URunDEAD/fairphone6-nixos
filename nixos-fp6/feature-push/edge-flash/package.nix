# Edge lighting: the com.fairphone6.edge-flash schema (for Lomiri's wrapper
# and System Settings) and the Settings > Edge Lighting page (pure QML).
{ lib, stdenvNoCC, glib }:
let
  schema = stdenvNoCC.mkDerivation {
    pname = "fp6-edge-flash-schema";
    version = "0.1.0";
    dontUnpack = true;
    nativeBuildInputs = [ glib ];
    installPhase = ''
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.edge-flash.gschema.xml} $schemas/com.fairphone6.edge-flash.gschema.xml
      glib-compile-schemas --strict $schemas
    '';
  };
  # Battery saver state for Fp6EdgeFlash.qml (the battery module installs
  # the same file for its daemon and Settings page).
  batterySchema = stdenvNoCC.mkDerivation {
    pname = "fp6-edge-flash-battery-schema";
    version = "0.1.0";
    dontUnpack = true;
    nativeBuildInputs = [ glib ];
    installPhase = ''
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${../battery/com.fairphone6.battery.gschema.xml} $schemas/com.fairphone6.battery.gschema.xml
      glib-compile-schemas --strict $schemas
    '';
  };
  lssPlugin = stdenvNoCC.mkDerivation {
    pname = "lomiri-system-settings-edge-lighting";
    version = "0.1.0";
    src = ./settings-page;
    nativeBuildInputs = [ glib ];
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      install -Dm644 edge-lighting.settings $out/share/lomiri-system-settings/edge-lighting.settings
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/edge-lighting *.qml
      # The wrapper lndir's these from every plugin.
      mkdir -p $out/lib/lomiri-system-settings $out/share/locale
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.edge-flash.gschema.xml} $schemas/com.fairphone6.edge-flash.gschema.xml
      glib-compile-schemas --strict $schemas
      runHook postInstall
    '';
    meta = {
      description = "Edge Lighting page (notification glow while the screen is off) for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit schema batterySchema lssPlugin; }
