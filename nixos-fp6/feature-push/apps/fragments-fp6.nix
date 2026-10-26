# Fragments (GNOME BitTorrent client, GTK4/libadwaita) for the Lomiri session.
#
#  * Lomiri's drawer lists only desktop entries with X-Lomiri-Touch=true; the
#    wrapper's copy adds it and points Exec at the wrapper.
#  * GTK_IM_MODULE=wayland: the session exports GTK_IM_MODULE=maliit, for which
#    there is no GTK module, so GTK would fall back to the simple context and
#    never request the on-screen keyboard. wayland = text-input-v3, which Mir
#    bridges to Maliit (see ../osk-wayland/).
#  * GDK_BACKEND=wayland keeps it off Xwayland (DISPLAY=:0 is also exported).
#  * Fragments starts transmission-daemon with its RPC bound to 0.0.0.0 even
#    with "remote access" off (only the 127.0.0.1/::1 whitelist protects it,
#    no password). The daemon is wrapped to append --rpc-bind-address
#    127.0.0.1 (the last occurrence wins), so it only listens on localhost.
#    Fragments' own "remote access" setting therefore has no effect.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.fragments;
  # Name keeps "-transmission" so the AppArmor profile's store glob matches.
  transmissionLocal = pkgs.symlinkJoin {
    name = "fp6-localhost-transmission";
    paths = [ pkgs.transmission_4 ];
    nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
    postBuild = ''
      rm "$out/bin/transmission-daemon"
      makeBinaryWrapper ${pkgs.transmission_4}/bin/transmission-daemon "$out/bin/transmission-daemon" \
        --append-flags "--rpc-bind-address 127.0.0.1"
    '';
  };
  base = pkgs.fragments.override { transmission_4 = transmissionLocal; };
  desktopId = "de.haeckerfelix.Fragments.desktop";
  wrapped = pkgs.symlinkJoin {
    name = "fragments-lomiri-fp6";
    paths = [ base ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      rm "$out/bin/fragments"
      makeWrapper ${base}/bin/fragments "$out/bin/fragments" \
        --set GDK_BACKEND wayland \
        --set GTK_IM_MODULE wayland
      src=$(readlink -f "$out/share/applications/${desktopId}")
      rm "$out/share/applications/${desktopId}"
      sed -e "s|^Exec=[^ ]*fragments|Exec=$out/bin/fragments|" \
          -e '/^\[Desktop Entry\]$/a X-Lomiri-Touch=true' \
          "$src" > "$out/share/applications/${desktopId}"
      grep -q "^Exec=$out/bin/fragments" "$out/share/applications/${desktopId}"
      test "$(grep -c '^X-Lomiri-Touch=true$' "$out/share/applications/${desktopId}")" = 1
    '';
    meta = base.meta or { };
  };
in {
  options.hardware.fairphone6.fragments.enable =
    lib.mkEnableOption "Fragments (BitTorrent) wrapped for Lomiri";

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ wrapped ];
    system.build.fp6-fragments = wrapped;
  };
}
