# The kernel5 full-features Lomiri target plus misc/quick-checks-fp6.nix
# (offline charging target, on-demand MTP, neard). Kept as a separate entry so
# the shared trial files are not edited while other agents work on them;
# merge the two lines below into trial-lomiri-full-fp6.nix when accepted.
# Build/eval only.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ./trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [ {
      imports = [ ./misc/quick-checks-fp6.nix ];
      hardware.fairphone6.quickChecks.enable = true;
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
}
