{ system ? "aarch64-linux", nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let pkgs = import nixpkgs { inherit system; };
in {
  manager = import ./default.nix { inherit pkgs; };
  cpu = import ./nix/llama.nix { inherit pkgs; };
  vulkan = import ./nix/llama.nix { inherit pkgs; vulkanSupport = true; };
  kleidi = import ./nix/llama.nix { inherit pkgs; kleidiAiSupport = true; };
}
