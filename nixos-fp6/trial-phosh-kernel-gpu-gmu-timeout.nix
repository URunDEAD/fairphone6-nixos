# Separate diagnostic GMU-timeout backport target; no automatic activation.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/gmu-timeout
}:
import ./trial-phosh-kernel-gpu-apparmor.nix { inherit nixpkgs kernelBuild; }
