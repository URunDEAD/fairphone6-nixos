{ lib, runCommand, makeWrapper, python3, dbus }:
let
  python = python3.withPackages (p: [ p.dbus-python p.pygobject3 ]);
in runCommand "fp6-lomiri-powerd-1" {
  nativeBuildInputs = [ makeWrapper ];
  passthru = { inherit python; };
  meta = {
    description = "FP6 repowerd-compatible display backend for Lomiri (manual/auto brightness, proximity, double-tap)";
    license = lib.licenses.mit;
    platforms = [ "aarch64-linux" ];
    mainProgram = "fp6-lomiri-powerd";
  };
} ''
  mkdir -p "$out/bin" "$out/lib/fp6-lomiri-powerd"
  cp ${./fp6-lomiri-powerd.py} "$out/lib/fp6-lomiri-powerd/fp6_lomiri_powerd.py"
  makeWrapper ${python}/bin/python3 "$out/bin/fp6-lomiri-powerd" \
    --add-flags "-W ignore::DeprecationWarning $out/lib/fp6-lomiri-powerd/fp6_lomiri_powerd.py"
  # Hardware-free D-Bus tests (fake backlight/touch sysfs + scripted SensorProxy).
  work=$(mktemp -d)
  cp ${./fp6-lomiri-powerd-tests.py} "$work/tests.py"
  cp ${./fp6-fake-sensorproxy.py} "$work/fake_sensorproxy.py"
  PYTHONPATH="$out/lib/fp6-lomiri-powerd" ${dbus}/bin/dbus-run-session \
    --dbus-daemon=${dbus}/bin/dbus-daemon --config-file=${dbus}/share/dbus-1/session.conf -- \
    ${python}/bin/python3 -W ignore "$work/tests.py" | tee "$out/lib/fp6-lomiri-powerd/test-log.txt"
  test "''${PIPESTATUS[0]}" = 0
''
