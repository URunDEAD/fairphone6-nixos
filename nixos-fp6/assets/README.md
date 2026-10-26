# nixos-fp6/assets (user-supplied, not in git)

The device definition (`nixos-fp6/device/`) still takes three inputs that are
binary, proprietary or device-derived. They are not redistributed here. Put
regular files (not symlinks: a symlink changes the Nix store path) with these
exact names into this directory:

| File | What it is | Where it comes from |
|---|---|---|
| `fp6-firmware.tar` | `firmware/` tree: Qualcomm/Fairphone firmware (`qcom/milos/fairphone/fp6/*.mbn`, ADSP/CDSP/modem/WPSS, `gen80300_zap.mbn`, `qca/msbtfw12.mbn`, ...) plus `firmware/postmarketos/*` | the postmarketOS FP6 firmware package (Catcrafts port), i.e. `/lib/firmware` of a working postmarketOS install on your phone: `tar -C /lib -cf fp6-firmware.tar firmware` |
| `fp6-kernel.tar` | `boot/vmlinuz`, `boot/milos-fairphone-fp6.dtb`, `usr/lib/modules/7.2.0/`, and the kernel config at `home/user/fp6-install/kernel-7.2.0.config` | the original postmarketOS kernel build (milos-linux 7.2.0). The current targets rebuild the kernel from source (`kernel-build/`), but the base device definition (`device/kernel.nix`) and the DTB repack still read this tarball. |
| `boot-template.img` | an Android boot image v2 of the phone (header and offsets are reused by `repack-boot.py`) | the working postmarketOS `boot.img` for your phone |

`kernel.config` (text) is tracked: it is the configuration of the original
kernel, which the kernel recipes overlay.

With other copies of these files the evaluation still works, but the system
and boot image derivation paths differ from the ones in the docs. The kernel
itself only depends on the pinned source archive and `config.final`, so the
`kernelPin.expectedKernelDrvPath` assertion in
`feature-push/trial-lomiri-full-kernel6-fp6.nix` holds as long as
`tools/fetch-sources.sh` reproduces the pinned archive hash.

Moving these into fetchable, hash-pinned sources (linux-firmware plus the
Fairphone vendor blobs, mainline DTB) is tracked in CONTRIBUTING-UPSTREAM.md
(Mobile NixOS device branch).
