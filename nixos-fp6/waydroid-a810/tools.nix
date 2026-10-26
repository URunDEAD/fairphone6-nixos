{ lib, stdenvNoCC, python3, makeWrapper, coreutils, util-linux, lxc, overlay }:
let
  trial = ./trial;
  runtimePath = lib.makeBinPath [ python3 coreutils util-linux lxc ];
in stdenvNoCC.mkDerivation {
  pname = "fp6-waydroid-fresh-trial-tools";
  version = "1";
  dontUnpack = true;
  nativeBuildInputs = [ makeWrapper ];
  installPhase = ''
    mkdir -p $out/bin $out/share/fp6-waydroid-a810/fresh-trial/baseline
    d=$out/share/fp6-waydroid-a810/fresh-trial
    cp ${trial + "/prepare-trial.py"} "$d/prepare-trial.py"
    cp ${trial + "/run-trial.py"} "$d/run-trial.py"
    cp ${trial + "/review-trial-config.py"} "$d/review-trial-config.py"
    cp ${trial + "/baseline/manifest.xml"} "$d/baseline/manifest.xml"
    cp ${trial + "/baseline/waydroid.seccomp"} "$d/baseline/waydroid.seccomp"
    # A copied runner retains a store Python shebang on NixOS.
    substituteInPlace "$d/run-trial.py" --replace-fail '#!/usr/bin/env python3' '#!${python3}/bin/python3'
    makeWrapper ${python3}/bin/python3 $out/bin/fp6-waydroid-prepare-trial \
      --prefix PATH : ${runtimePath} --add-flags "$d/prepare-trial.py" \
      --add-flags "--mesa-tree ${overlay}/lib64" \
      --add-flags "--mesa-manifest ${overlay}/share/fp6-waydroid-a810/artifacts.json"
    makeWrapper ${python3}/bin/python3 $out/bin/fp6-waydroid-review-trial \
      --prefix PATH : ${runtimePath} --add-flags "$d/review-trial-config.py"
    # The trial-local runner enforces its exact path, fresh data and resource gates.
    cat > $out/bin/fp6-waydroid-run-trial <<'SH'
    #!${stdenvNoCC.shell}
    set -eu
    case "''${1:-}" in
      /var/lib/fp6-waydroid-trials/fp6-*) trial="$1"; shift ;;
      *) echo 'Usage: fp6-waydroid-run-trial /var/lib/fp6-waydroid-trials/fp6-NAME --start-180s' >&2; exit 2 ;;
    esac
    export PATH=${runtimePath}
    exec ${python3}/bin/python3 "$trial/run-trial.py" "$@"
    SH
    chmod +x $out/bin/fp6-waydroid-run-trial
  '';
  meta = {
    description = "Explicit preparation/review/bounded runtime tools for new private Android trials";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
