{ pkgs, ... }:
{
  imports = [ ./audio-fp6.nix ./bluetooth-fp6.nix ./lomiri-fp6-apps.nix ./torch-fp6.nix ];
  hardware.fp6BluetoothIdentity.enable = true;
  hardware.fp6Torch.enable = true;
  # Mir 2.15 needs legacy Wayland import; AArch64 llvmpipe must only use
  # extensions exposed by the OS, rather than the CPU model's optional SVE.
  hardware.graphics.package = pkgs.mesa.overrideAttrs (old: {
    patches = (old.patches or []) ++ [ ./mesa-aarch64-os-features.patch ];
    mesonFlags = (old.mesonFlags or []) ++ [ "-Dlegacy-wayland=bind-wayland-display" ];
  });
  # Keep the Wi-Fi SSH connection available while USB is used for charging.
  networking.networkmanager.wifi.powersave = false;
}
