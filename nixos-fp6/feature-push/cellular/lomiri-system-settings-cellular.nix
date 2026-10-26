# System Settings > Cellular (UBports lomiri-system-settings-cellular 1.3).
# Not packaged in nixpkgs: upstream split the panel out of
# lomiri-system-settings in 2021. Built as a wrapper plugin
# (pkgs.lomiri.lomiri-system-settings.override { plugins = [ ... ]; }).
#
# It talks to oFono directly through QOfono (RadioSettings "Connection
# type", ConnectionManager contexts/APNs, roaming) and to
# lomiri-indicator-network (Lomiri.Connectivity: mobile data switch, SIM for
# data). "Connection type" lists RadioSettings.AvailableTechnologies with
# labels gsm "2G only", umts "2G/3G", lte "2G/3G/4G", nr "2G/3G/4G/5G"
# (needs ofono-0001-nr-5g.patch for "nr").
#
# Local changes:
#  * install paths: pkg-config's plugin_* variables point into the unwrapped
#    lomiri-system-settings store path; they are rebased onto $out.
#  * OfonoMtkSettings (UBports QOfono fork only) -> inert QtObject.
#  * Lomiri.Keyboard 0.1 (lomiri-keyboard) is not in nixpkgs (maliit is used
#    instead); the APN editor's text field only used it for the "Next"
#    enter-key label, so that import/hint is dropped.
{ lib, stdenv, fetchFromGitLab, cmake, pkg-config, gettext, python3
, qtbase, qtdeclarative, wrapQtAppsHook
, lomiri-system-settings-unwrapped, gsettings-qt }:

stdenv.mkDerivation (finalAttrs: {
  pname = "lomiri-system-settings-cellular";
  version = "1.3";

  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/lomiri-system-settings-cellular";
    rev = finalAttrs.version; # a988d0e128c47d931ab64cfda02efb83983ac682
    hash = "sha256-m1NXO8XchYD/vp0U44mzHtaj1zpggf2VO0/nkyolrFc=";
  };

  postPatch = ''
    for v in PLUGIN_MANIFEST_DIR PLUGIN_PRIVATE_MODULE_DIR PLUGIN_MODULE_DIR PLUGIN_QML_DIR; do
      echo "string(REPLACE \"${lomiri-system-settings-unwrapped}\" \"\''${CMAKE_INSTALL_PREFIX}\" $v \"\''${$v}\")" >> rebase.cmake
    done
    substituteInPlace CMakeLists.txt \
      --replace-fail 'add_subdirectory(plugins/cellular)' 'include(''${CMAKE_SOURCE_DIR}/rebase.cmake)
    add_subdirectory(plugins/cellular)'
    # OfonoMtkSettings (MediaTek 3G switch) exists only in UBports' QOfono
    # fork; nixpkgs' libqofono lacks it and the whole SIM page failed to
    # load ("No SIM"). Replace it with an inert stand-in (has3G = true).
    python3 - <<'PY'
    import re
    p = "plugins/cellular/Components/Sim.qml"
    s = open(p).read()
    s, n = re.subn(r"    OfonoMtkSettings \{.*?\n    \}\n", "    QtObject {\n        id: mtkSettings\n        property bool _valid: false\n        property bool has3G: true\n    }\n", s, flags=re.S)
    assert n == 1, n
    open(p, "w").write(s)
    PY
    substituteInPlace plugins/cellular/Components/LabelTextField.qml \
      --replace-fail 'import Lomiri.Keyboard 0.1' "" \
      --replace-fail 'InputMethod.extensions: { "enterKeyText": I18nd.tr("Next") }' ""
  '';

  strictDeps = true;

  nativeBuildInputs = [ cmake pkg-config gettext python3 qtdeclarative wrapQtAppsHook ];

  buildInputs = [ qtbase qtdeclarative lomiri-system-settings-unwrapped ];

  # QML modules the panel imports at runtime (QOfono, Lomiri.Connectivity,
  # QMenuModel, Lomiri.Components come with lomiri-system-settings-unwrapped).
  propagatedBuildInputs = [ gsettings-qt ];

  dontWrapQtApps = true;

  postInstall = ''
    test -e $out/share/lomiri-system-settings/cellular.settings
    test -e $out/lib/lomiri-system-settings/libcellular-plugin.so
    test -d $out/share/lomiri-system-settings/qml-plugins/cellular || test -n "$(find $out -name PageComponent.qml)"
  '';

  meta = {
    description = "Cellular settings plugin for Lomiri's System Settings";
    homepage = "https://gitlab.com/ubports/development/core/lomiri-system-settings-cellular";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
})
