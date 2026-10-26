# Package the upstream Phone Tour app without enabling first-login onboarding.
{ lib, stdenv, fetchurl, meson, ninja, pkg-config, gettext, desktop-file-utils
, wrapGAppsHook4, gtk4, libadwaita, gmobile }:
stdenv.mkDerivation {
  pname = "phosh-tour";
  version = "0.57.0";

  src = fetchurl {
    url = "https://gitlab.gnome.org/World/Phosh/phosh-tour/-/archive/v0.57.0/phosh-tour-v0.57.0.tar.gz";
    sha256 = "96cadbda7367304233e90084da544f546ade397a8b4464fe99882e7ad00a06b1";
  };

  nativeBuildInputs = [ meson ninja pkg-config gettext desktop-file-utils wrapGAppsHook4 ];
  buildInputs = [ gtk4 libadwaita gmobile ];
  strictDeps = true;
  doCheck = true;

  # Installing a tour should expose its launcher, not change login behavior.
  # Upstream's optional first-login service also assumes /usr/bin/phosh-tour.
  postInstall = ''
    rm -f "$out/etc/xdg/autostart/mobi.phosh.PhoshTour-first-login.desktop"
    rm -f "$out/lib/systemd/user/mobi.phosh.PhoshTour-first-login.service"
    # Phosh is not registered in desktop-file-utils' OnlyShowIn list. Make
    # the manual tour launcher available in all desktops instead.
    sed -i '/^OnlyShowIn=/d' "$out/share/applications/mobi.phosh.PhoshTour.desktop"
    desktop-file-validate "$out/share/applications/mobi.phosh.PhoshTour.desktop"
  '';

  meta = {
    description = "Phone Tour introduction to Phosh";
    homepage = "https://gitlab.gnome.org/World/Phosh/phosh-tour";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "phosh-tour";
  };
}
