# Current upstream Chatty with a real optional Matrix backend. This package
# selects native SMS/MMS only; no libcmatrix/libolm or insecure exceptions.
{ lib, stdenv, fetchurl, meson, ninja, pkg-config, gettext, itstool, python3
, desktop-file-utils, wrapGAppsHook4, gtk4, libadwaita, evolution-data-server
, gnome-desktop, gtksourceview5, gst_all_1, libphonenumber, modemmanager
, gsettings-desktop-schemas, sqlite, protobuf, libxml2, gnupg, dbus, xvfb-run }:
stdenv.mkDerivation {
  pname = "chatty-sms-mms";
  version = "0.8.9";
  src = fetchurl {
    url = "https://gitlab.gnome.org/World/Chatty/-/archive/v0.8.9/Chatty-v0.8.9.tar.gz";
    sha256 = "bcca5e749e4a83035af18cc3bba56c83ecf589d7fd9a281f0c6a1a9a519d0e06";
  };
  patches = [ ./chatty-optional-matrix.patch ./chatty-history-test-finalize.patch ];
  nativeBuildInputs = [ meson ninja pkg-config gettext itstool python3 libxml2 desktop-file-utils wrapGAppsHook4 ];
  buildInputs = [ gtk4 libadwaita evolution-data-server gnome-desktop gtksourceview5
    gst_all_1.gstreamer libphonenumber modemmanager gsettings-desktop-schemas sqlite protobuf ];
  nativeCheckInputs = [ dbus xvfb-run gnupg ];
  mesonFlags = [ "-Dmatrix=disabled" "-Dpurple=disabled" "-Dgtk_doc=false" ];
  strictDeps = true;
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export HOME="$TMPDIR/chatty-test-home"
    mkdir -p "$HOME"
    export XDG_RUNTIME_DIR="$TMPDIR/chatty-test-runtime"
    mkdir -m700 "$XDG_RUNTIME_DIR"
    export XDG_DATA_DIRS="${gsettings-desktop-schemas}/share/gsettings-schemas/${gsettings-desktop-schemas.name}''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
    export GSK_RENDERER=cairo GTK_A11Y=none NO_AT_BRIDGE=1
    dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- xvfb-run -a meson test --print-errorlogs
    runHook postCheck
  '';
  preFixup = ''
    gappsWrapperArgs+=(--prefix PATH : ${lib.makeBinPath [ gnupg ]})
  '';
  postInstall = ''
    # Installing the app must not start a messaging daemon in a user session.
    rm -f "$out/etc/xdg/autostart/sm.puri.Chatty-daemon.desktop"
    rm -f "$out/lib/systemd/user/sm.puri.Chatty-daemon.service"
    python3 - <<'PYMETA'
import os
from pathlib import Path
import xml.etree.ElementTree as ET
out = Path(os.environ['out'])
desktop = out / 'share/applications/sm.puri.Chatty.desktop'
lines = [line for line in desktop.read_text().splitlines()
         if not line.startswith(('Comment=', 'Comment[', 'Keywords=', 'Keywords['))]
lines += ['Comment=SMS/MMS messaging (Matrix and XMPP disabled)',
          'Keywords=SMS;MMS;chat;messaging;modem;']
desktop.write_text('\n'.join(lines) + '\n')
metainfo = out / 'share/metainfo/sm.puri.Chatty.metainfo.xml'
tree = ET.parse(metainfo)
root = tree.getroot()
for description in root.findall('description'):
    root.remove(description)
description = ET.SubElement(root, 'description')
ET.SubElement(description, 'p').text = 'Chatty SMS/MMS messaging. Matrix and XMPP are not included in this build.'
tree.write(metainfo, encoding='utf-8', xml_declaration=True)
PYMETA
    desktop-file-validate "$out/share/applications/sm.puri.Chatty.desktop"
  '';
  meta = {
    description = "Chatty SMS/MMS messaging frontend (Matrix and XMPP disabled)";
    homepage = "https://gitlab.gnome.org/World/Chatty";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "chatty";
  };
}
