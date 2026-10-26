# Frontend only; no vvmd/ModemManager service or voicemail/account access.
{ lib, stdenv, fetchurl, meson, ninja, pkg-config, gettext, desktop-file-utils
, wrapGAppsHook4, gtk4, libadwaita, evolution-data-server, gst_all_1, callaudiod }:
stdenv.mkDerivation {
  pname = "vvmplayer";
  version = "2.6";
  src = fetchurl {
    url = "https://gitlab.com/kop316/vvmplayer/-/archive/2.6/vvmplayer-2.6.tar.gz";
    sha256 = "49869baa9d063b88b6c09596df65f794e403ea1f80cc49455535be07071b67a1";
  };
  patches = [ ./postmarketos-apps-vvmplayer-eds360.patch ];
  nativeBuildInputs = [ meson ninja pkg-config gettext desktop-file-utils wrapGAppsHook4 ];
  buildInputs = [ gtk4 libadwaita evolution-data-server gst_all_1.gstreamer gst_all_1.gst-plugins-base callaudiod ];
  # Package installation must not start voicemail polling in a user session.
  postInstall = ''
    rm -f "$out/etc/xdg/autostart/org.kop316.vvmplayer-daemon.desktop"
    rmdir --ignore-fail-on-non-empty "$out/etc/xdg/autostart" "$out/etc/xdg" "$out/etc"
  '';
  doCheck = true;
  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    desktop-file-validate "$out/share/applications/"*.desktop
    "$out/bin/vvmplayer" --help > /dev/null
    runHook postInstallCheck
  '';
  meta = {
    description = "Visual Voicemail Player frontend; service/account integration remains separate";
    homepage = "https://gitlab.com/kop316/vvmplayer";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "vvmplayer";
  };
}
