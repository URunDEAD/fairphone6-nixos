#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
test "$(git -C source rev-parse HEAD)" = 63289f4411520a2acb38bd533437446d33fbce74
test -z "$(git -C source status --porcelain)"
printf '%s  %s\n' ac3130009fe160c7a0235350b08a2b8f554a9285aee1d667f0c6434df6c6ee4c ../nixos-fp6/assets/kernel.config | sha256sum -c -
mkdir -p config-work
cp ../nixos-fp6/assets/kernel.config config-work/.config
KCONFIG_CONFIG="$PWD/config-work/.config" bash source/scripts/kconfig/merge_config.sh -m -r config-work/.config features.config
# Run in the pinned LLVM development shell, on any Linux build host.
# This compiles only the small host Kconfig tools, never the kernel.
make -C source O="$PWD/config-work" ARCH=arm64 LLVM=1 olddefconfig
cp config-work/.config config.final
python3 verify-config.py config.final
python3 source/scripts/diffconfig ../nixos-fp6/assets/kernel.config config.final > config-diff.txt
