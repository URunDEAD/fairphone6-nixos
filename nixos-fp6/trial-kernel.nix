# AppArmor/binderfs RAM-test build. Never writes phone partitions.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build
, extraModules ? []
}:
let
  # Keep the kernel recipe's exact release archive pin, matching the builder.
  kernel = import kernelBuild { system = "aarch64-linux"; };
  # Copy the script into the store even when kernelBuild is an absolute
  # string supplied by an external builder expression.
  repacker = builtins.path {
    path = kernelBuild + "/repack-new-kernel.py";
    name = "fp6-repack-new-kernel.py";
  };
in import ./upstream/mobile-nixos {
  pkgs = import nixpkgs { system = "aarch64-linux"; config.allowUnfree = true; };
  device = ./device;
  configuration = { config, lib, pkgs, ... }: {
    imports = [ ./configuration.nix ./hardware-fp6.nix ] ++ extraModules;
    mobile.boot.stage-1.kernel.package = lib.mkForce kernel;
    # Real module/config checks remain mandatory: Mobile's custom kernel path
    # disables NixOS's requiredKernelConfig assertions.
    security.apparmor.enable = true;
    virtualisation.waydroid.enable = true;
    environment.systemPackages = [ pkgs.python3 ];
    # Validate the kernel and GUI first. Android image initialization is an
    # explicit later step using a separate data directory.
    systemd.services = lib.mkIf config.virtualisation.waydroid.enable {
      waydroid-container.wantedBy = lib.mkForce [];
    };
    # Override the device's mkForce output, which preserves the original kernel.
    mobile.outputs.android.android-bootimg = lib.mkOverride 40 (pkgs.runCommand "fp6-nixos-kernel-trial.img" {
      nativeBuildInputs = [ pkgs.python3 ];
    } ''
      python ${repacker} ${./assets/boot-template.img} ${kernel}/Image.gz \
        ${config.mobile.outputs.initrd} ${kernel}/dtbs/qcom/milos-fairphone-fp6.dtb $out \
        --cmdline ${lib.escapeShellArg (lib.concatStringsSep " " config.boot.kernelParams)}
    '');
  };
}
