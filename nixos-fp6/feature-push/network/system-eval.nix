# Evaluate the real FP6 configuration with the network module enabled.
#   nix-instantiate --eval --strict --json system-eval.nix -A summary
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  mobile = import ../../upstream/mobile-nixos {
    pkgs = import nixpkgs { system = "aarch64-linux"; config.allowUnfree = true; };
    device = ../../device;
    configuration = {
      imports = [ ../../configuration.nix ./network-fp6.nix ];
      hardware.fairphone6.network.enable = true;
    };
  };
  c = mobile.config;
in
{
  toplevel = c.system.build.toplevel;
  summary = {
    toplevelDrv = c.system.build.toplevel.drvPath;
    ayatanaIndicators = map (p: p.pname or p.name) c.services.ayatana-indicators.packages;
    lomiri = c.services.desktopManager.lomiri.enable;
    bluetooth = c.hardware.bluetooth.enable;
    pipewire = c.services.pipewire.enable;
    fp6BluetoothIdentity = c.hardware.fp6BluetoothIdentity.enable or "module-not-imported";
    firewall = c.networking.firewall.interfaces;
    urfkill = c.systemd.services.fp6-urfkill.serviceConfig.ExecStart;
  };
}
