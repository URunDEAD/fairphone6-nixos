_final: prev: {
  lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
    lomiri-telephony-service = lprev.lomiri-telephony-service.overrideAttrs (old: {
      postInstall = (old.postInstall or "") + ''
        # Both installed handler D-Bus names request this user unit.
        install -Dm644 ../debian/lomiri-telephony-service.lomiri-telephony-service-handler.user.service \
          "$out/lib/systemd/user/lomiri-telephony-service-handler.service"
        # systemd accepts one scalar BusName; keep upstream's effective name.
        substituteInPlace "$out/lib/systemd/user/lomiri-telephony-service-handler.service" \
          --replace-fail '/usr/bin/' "$out/bin/" \
          --replace-fail 'BusName=com.lomiri.TelephonyServiceHandler' ""
        # Fix the fallback activation executable's obsolete spelling.
        substituteInPlace "$out/share/dbus-1/services/org.freedesktop.Telepathy.Client.TelephonyServiceIndicator.service" \
          --replace-fail '/lomiri-telephony-service-indicator' '/lomiri-indicator-telephony-service'
      '';
    });
  });
}
