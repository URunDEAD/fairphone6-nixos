# Optional runtime integration for the migrated native application launchers.
# No catalog remote, account, Android initialization or application is started.
{ config, lib, ... }:
let cfg = config.hardware.fairphone6.postmarketosApps.services;
in {
  options.hardware.fairphone6.postmarketosApps.services.enable = lib.mkEnableOption
    "Flatpak/Software and parental-controls runtime integration for the native apps";
  config = lib.mkIf cfg.enable {
    # The pinned NixOS PackageKit backend is test_nop, so it cannot provide
    # working Nix installs. Flatpak supplies the supported Software backend.
    # Repositories are a separate explicit setup step; none are added here.
    services.flatpak.enable = true;
    services.gnome.gnome-software.enable = true;
    # malcontent-ui alone does not install the system AccountsService extension.
    services.malcontent.enable = true;
  };
}
