# com.fairphone6.lockscreen GSettings schema (lockscreen-polish module). The
# sensor geometry defaults come from the Nix options, so the schema is
# generated. Read by Lomiri (greeter hint, home-wallpaper effects) and the
# Background settings page.
{ stdenvNoCC, glib, sensorCenter, sensorLength }:
let
  fmt = x: builtins.toString x;
in stdenvNoCC.mkDerivation {
  name = "fp6-lockscreen-schema-1";
  dontUnpack = true;
  nativeBuildInputs = [ glib ];
  xml = ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!-- FP6 lock screen and wallpapers (nixos-fp6/feature-push/lockscreen-polish). -->
    <schemalist gettext-domain="lomiri-system-settings">
      <schema id="com.fairphone6.lockscreen" path="/com/fairphone6/lockscreen/">
        <key name="fingerprint-sensor-center" type="d">
          <range min="0.05" max="0.95"/>
          <default>${fmt sensorCenter}</default>
          <summary>Centre of the power-button fingerprint sensor, fraction of the portrait screen height from the top</summary>
        </key>
        <key name="fingerprint-sensor-length" type="d">
          <range min="0.02" max="0.3"/>
          <default>${fmt sensorLength}</default>
          <summary>Length of the edge hint, fraction of the portrait screen height</summary>
        </key>
        <key name="fingerprint-hint-text" type="b">
          <default>true</default>
          <summary>Show "Touch the power button to unlock" briefly when the lock screen appears</summary>
        </key>
        <key name="fingerprint-haptics" type="b">
          <default>true</default>
          <summary>Vibrate when a finger is not recognised (also follows the system vibration setting)</summary>
        </key>
        <key name="home-wallpaper-dim" type="d">
          <range min="0" max="0.8"/>
          <default>0.0</default>
          <summary>Dim the home-screen wallpaper (0 = off)</summary>
        </key>
        <key name="home-wallpaper-blur" type="b">
          <default>false</default>
          <summary>Blur the home-screen wallpaper</summary>
        </key>
      </schema>
    </schemalist>
  '';
  passAsFile = [ "xml" ];
  installPhase = ''
    schemas=$out/share/gsettings-schemas/$name/glib-2.0/schemas
    mkdir -p $schemas
    cp $xmlPath $schemas/com.fairphone6.lockscreen.gschema.xml
    glib-compile-schemas --strict $schemas
  '';
}
