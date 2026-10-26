{ lib, runCommand, makeWrapper, python3, dbus }:
let
  python = python3.withPackages (p: [ p.dbus-python p.pygobject3 ]);
in runCommand "fp6-brightness-dbus-1" {
  nativeBuildInputs = [ makeWrapper ];
  passthru = { inherit python; };
  meta = {
    description = "Manual FP6 backlight bridge for Lomiri's power indicator";
    license = lib.licenses.mit;
    platforms = [ "aarch64-linux" ];
  };
} ''
  mkdir -p "$out/bin" "$out/lib/fp6-brightness"
  cp ${./fp6-brightness-dbus.py} "$out/lib/fp6-brightness/fp6_brightness.py"
  makeWrapper ${python}/bin/python3 "$out/bin/fp6-brightness-dbus" \
    --add-flags "$out/lib/fp6-brightness/fp6_brightness.py"
  PYTHONPATH="$out/lib/fp6-brightness" ${dbus}/bin/dbus-run-session \
    --dbus-daemon=${dbus}/bin/dbus-daemon --config-file=${dbus}/share/dbus-1/session.conf -- \
    ${python}/bin/python3 ${./fp6-brightness-tests.py}
''
