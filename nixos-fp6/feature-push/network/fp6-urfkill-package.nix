# URfkill-compatible flight-mode daemon for Lomiri on the FP6.
{ lib, stdenvNoCC, python3, makeWrapper, glib, gobject-introspection, dbus }:
let
  py = python3.withPackages (ps: [ ps.dbus-python ps.pygobject3 ]);
in
stdenvNoCC.mkDerivation {
  pname = "fp6-urfkill";
  version = "0.1";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./fp6-urfkill.py ./fp6-urfkill-check.py ];
  };
  nativeBuildInputs = [ makeWrapper ];
  nativeInstallCheckInputs = [ dbus py ];
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    install -Dm0644 fp6-urfkill.py $out/libexec/fp6-urfkill/fp6-urfkill.py
    makeWrapper ${py}/bin/python3 $out/bin/fp6-urfkill \
      --add-flags "-I $out/libexec/fp6-urfkill/fp6-urfkill.py" \
      --prefix GI_TYPELIB_PATH : "${lib.getLib glib}/lib/girepository-1.0:${lib.getLib gobject-introspection}/lib/girepository-1.0"

    install -Dm0644 /dev/stdin $out/share/dbus-1/system-services/org.freedesktop.URfkill.service <<EOF
    [D-BUS Service]
    Name=org.freedesktop.URfkill
    Exec=/run/current-system/sw/bin/false
    User=root
    SystemdService=fp6-urfkill.service
    EOF

    install -Dm0644 /dev/stdin $out/share/dbus-1/system.d/org.freedesktop.URfkill.conf <<EOF
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="org.freedesktop.URfkill"/>
      </policy>
      <!-- Callers may talk to the service; mutating methods are gated by polkit inside the daemon. -->
      <policy context="default">
        <allow send_destination="org.freedesktop.URfkill"/>
      </policy>
    </busconfig>
    EOF

    install -Dm0644 /dev/stdin $out/share/polkit-1/actions/org.freedesktop.urfkill.policy <<EOF
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <vendor>FP6 NixOS</vendor>
      <action id="org.freedesktop.urfkill.block">
        <description>Block or unblock radio devices</description>
        <message>Authentication is required to change radio kill switches</message>
        <defaults>
          <allow_any>no</allow_any>
          <allow_inactive>no</allow_inactive>
          <allow_active>yes</allow_active>
        </defaults>
      </action>
      <action id="org.freedesktop.urfkill.flightmode">
        <description>Enable or disable flight mode</description>
        <message>Authentication is required to change flight mode</message>
        <defaults>
          <allow_any>no</allow_any>
          <allow_inactive>no</allow_inactive>
          <allow_active>yes</allow_active>
        </defaults>
      </action>
    </policyconfig>
    EOF
    runHook postInstall
  '';
  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    DBUS_SESSION_CONF=${dbus.out}/share/dbus-1/session.conf \
      ${py}/bin/python3 -I fp6-urfkill-check.py $out/bin/fp6-urfkill
    runHook postInstallCheck
  '';
  meta = {
    description = "Minimal org.freedesktop.URfkill provider (rfkill + modem offline) for Lomiri flight mode";
    platforms = lib.platforms.linux;
    mainProgram = "fp6-urfkill";
  };
}
