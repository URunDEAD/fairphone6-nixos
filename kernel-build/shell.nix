{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }
}:
let pkgs = import nixpkgs {};
in pkgs.mkShell {
  packages = with pkgs; [ llvmPackages.clang llvmPackages.lld llvmPackages.llvm bison flex gnumake python3 git openssl pahole ];
}
