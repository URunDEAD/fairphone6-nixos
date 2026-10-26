# NetworkManager VPN plugin "netbird" (nm-netbird-service.py), packaged the way
# networking.networkmanager.plugins expects: lib/NetworkManager/VPN/*.name plus
# its D-Bus policy in share/dbus-1/system.d.
{ pkgs, netbird }:
let
  inherit (pkgs) lib;
  service-bin = pkgs.writers.writePython3Bin "nm-netbird-service" {
    libraries = [ pkgs.python3Packages.pygobject3 ];
    flakeIgnore = [ "E501" "W503" "E731" ];
  } (builtins.readFile ./nm-netbird-service.py);
in rec {
  networkmanager-netbird = pkgs.runCommand "networkmanager-fp6-netbird-0.1" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    passthru.networkManagerPlugin = "VPN/nm-netbird-service.name";
    meta.mainProgram = "nm-netbird-service";
  } ''
    makeWrapper ${service-bin}/bin/nm-netbird-service $out/libexec/nm-netbird-service \
      --prefix GI_TYPELIB_PATH : ${lib.makeSearchPath "lib/girepository-1.0" [ pkgs.glib.out pkgs.gobject-introspection ]} \
      --set FP6_NETBIRD_BIN ${lib.getExe netbird} \
      --set FP6_SETPRIV_BIN ${pkgs.util-linux}/bin/setpriv \
      --set-default NB_STATE_DIR /var/lib/netbird \
      --set-default NB_DAEMON_ADDR unix:///var/run/netbird/sock

    mkdir -p $out/lib/NetworkManager/VPN $out/share/dbus-1/system.d
    cat > $out/lib/NetworkManager/VPN/nm-netbird-service.name <<EOF
    [VPN Connection]
    name=netbird
    service=org.freedesktop.NetworkManager.netbird
    program=$out/libexec/nm-netbird-service
    supports-multiple-connections=false
    EOF

    cat > $out/share/dbus-1/system.d/nm-netbird-service.conf <<'EOF'
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own_prefix="org.freedesktop.NetworkManager.netbird"/>
        <allow send_destination="org.freedesktop.NetworkManager.netbird"/>
      </policy>
      <policy context="default">
        <deny own_prefix="org.freedesktop.NetworkManager.netbird"/>
        <deny send_destination="org.freedesktop.NetworkManager.netbird"/>
      </policy>
    </busconfig>
    EOF
  '';
}
