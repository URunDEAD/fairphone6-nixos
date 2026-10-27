let
  pkgs = import <nixpkgs> {};
  evaluate = enabled: import (<nixpkgs> + "/nixos/lib/eval-config.nix") {
    system = "x86_64-linux";
    modules = [ ../module.nix {
      system.stateVersion = "26.05";
      hardware.fairphone6.webshare = {
        enable = enabled;
        package = pkgs.hello; # Evaluate integration without building the app.
        openFirewallInterfaces = [ "wlan0" "ap0" ];
      };
    } ];
  };
  on = (evaluate true).config;
  off = (evaluate false).config;
  cross = import <nixpkgs> { system = "x86_64-linux"; crossSystem.config = "aarch64-unknown-linux-gnu"; };
in
assert on.systemd.user.services.fp6-webshare.serviceConfig.Type == "dbus";
assert on.systemd.user.services.fp6-webshare.serviceConfig.BusName == "com.fairphone6.WebShare1";
assert on.networking.firewall.interfaces.wlan0.allowedTCPPorts == [ 8765 ];
assert on.networking.firewall.interfaces.ap0.allowedTCPPorts == [ 8765 ];
assert !(builtins.elem 8765 on.networking.firewall.allowedTCPPorts);
assert !(builtins.hasAttr "fp6-webshare" off.systemd.user.services);
assert builtins.elem "/share/lomiri-content-hub/peers" on.environment.pathsToLink;
{
  moduleChecks = "passed";
  aarch64Derivation = (cross.callPackage ../package.nix {}).drvPath;
}
