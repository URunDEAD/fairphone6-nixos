# New-kernel UI trial. Build only; deployment/boot remain explicit operations.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build
}:
import ./trial-kernel.nix {
  inherit nixpkgs kernelBuild;
  extraModules = [
    ./qt5-borderimage-native.nix
    ./power-fp6.nix
    ./sensors-fp6.nix
    ./power-policy-fp6.nix
    ./waydroid-a810.nix
    ({ lib, ... }: {
      # Install only the reviewed fresh-trial tools; no stock container/start.
      virtualisation.waydroid.enable = lib.mkForce false;
      hardware.fairphone6.androidMesaTrial.enable = true;
      services.fp6ManualBrightness.enable = true;
      hardware.fp6IioSensors.enable = true;
      services.fp6PhonePowerPolicy.enable = true;
    })
  ];
}
