# Opt-in exposure only: no service, container, image mutation or automatic start.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.androidMesaTrial;
  packages = import ./waydroid-a810 { inherit pkgs; };
  tools = pkgs.callPackage ./waydroid-a810/tools.nix { overlay = cfg.package; };
in {
  options.hardware.fairphone6.androidMesaTrial = {
    enable = lib.mkEnableOption "the reviewed A810 seven-library package and fresh-trial tools";
    package = lib.mkOption {
      type = lib.types.package;
      default = packages.runtime;
      defaultText = lib.literalExpression "(import ./waydroid-a810 { inherit pkgs; }).runtime";
      description = ''
        Matched Android API33 Bionic vendor overlay from pinned Nix sources,
        excluding build-only compiler/NDK metadata. The independent hash-pinned
        Clang18 repack remains available as packages.binary. This option never
        applies the overlay or claims physical GPU acceptance.
      '';
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = pkgs.stdenv.hostPlatform.system == "aarch64-linux";
        message = "FP6 Android Mesa trials target aarch64-linux only."; }
      { assertion = !config.virtualisation.waydroid.enable;
        message = "Fresh A810 trials require the stock Waydroid service disabled; use only the reviewed private trial."; }
    ];
    environment.etc."fp6-waydroid-a810".source = cfg.package;
    # LXC CLI availability also makes the documented exact-name stop accessible.
    # Installing this package does not enable virtualisation.lxc or lxc-net.
    environment.systemPackages = [ tools pkgs.lxc ];
  };
}
