#!/usr/bin/env bash
# Builds the package for the laptop (x86) and renders previews:
#   test/run-preview.sh OUTDIR tool[:page[:theme]] ...
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
N=${NIXPKGS:-$(nix-instantiate --eval -E '(import '"$here"'/../../upstream/mobile-nixos/npins).nixpkgs.outPath' | tr -d '"')}
out=$1; shift
mkdir -p "$out"
P=$(nix-build --no-out-link -E "(import $N {}).callPackage $here/package.nix {}")
export SURU_ICONS=$(nix-build --no-out-link -E "(import $N {}).lomiri.suru-icon-theme")
for spec in "$@"; do
  IFS=: read -r tool page th <<<"$spec"
  theme=Lomiri.Components.Themes.Ambiance
  [ "${th:-}" = dark ] && theme=Lomiri.Components.Themes.SuruDark
  name=$tool${page:+-$page}${th:+-$th}
  nix-shell -I nixpkgs=$N -p xvfb-run mesa --run "$here/test/preview.sh $P $tool $out/$name.png '${page:-}' $theme" 2>&1 \
    | grep -v -i fontconfig | grep -v '^\s*/' | grep -v '^$' || true
done
