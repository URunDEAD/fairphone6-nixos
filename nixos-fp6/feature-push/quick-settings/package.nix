# FP6 quick settings: the GSettings schema (for Lomiri's wrapper and the CLI),
# the System Settings page and the dark-theme helper.
{ lib, stdenvNoCC, glib, writeShellApplication, dconf, inotify-tools, coreutils, gnugrep, gawk }:
let
  schema = stdenvNoCC.mkDerivation {
    pname = "fp6-quick-settings-schema";
    version = "0.1.0";
    dontUnpack = true;
    nativeBuildInputs = [ glib ];
    installPhase = ''
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.quick-settings.gschema.xml} $schemas/com.fairphone6.quick-settings.gschema.xml
      glib-compile-schemas --strict $schemas
    '';
  };
  lssPlugin = stdenvNoCC.mkDerivation {
    pname = "lomiri-system-settings-quick-settings";
    version = "0.1.0";
    src = ./settings-page;
    nativeBuildInputs = [ glib ];
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      runHook preInstall
      install -Dm644 quick-settings.settings $out/share/lomiri-system-settings/quick-settings.settings
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/quick-settings *.qml
      # The wrapper lndir's these from every plugin.
      mkdir -p $out/lib/lomiri-system-settings $out/share/locale
      schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
      install -Dm644 ${./com.fairphone6.quick-settings.gschema.xml} $schemas/com.fairphone6.quick-settings.gschema.xml
      glib-compile-schemas --strict $schemas
      runHook postInstall
    '';
    meta = {
      description = "Quick settings page (control centre tiles, sliders) for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
  themeHelper = writeShellApplication {
    name = "fp6-quick-settings-theme";
    runtimeInputs = [ dconf inotify-tools coreutils gnugrep gawk ];
    text = builtins.readFile ./fp6-quick-settings-theme.sh;
    # grep/read return non-zero by design: nounset only (no errexit/pipefail).
    bashOptions = [ "nounset" ];
  };
in { inherit schema lssPlugin themeHelper; }
