S5KKD1 opt-in boot DTB source, exact kernel3 only.

candidate-preserved.pp.dts is the full preprocessed, source-level device tree
used by the independently audited preserved-symbol candidate. It contains the
original exact kernel3 board tree, all377original phandle assignments, and only
the front-camera additions from results/fp6-selfie-source-review/candidate/.
Its SHA256 is124f95cdfcb1982eec22498a64c09f70e5544919f7944eacd39977730ebce6c4.
No device/private data was extracted to produce it.

The package ../selfie-fp6-boot-dtb.nix compiles it using the exact kernel3
embedded DTC and Kbuild flags including -@. It verifies the original kernel DTB
hashb5f594... and requires the output SHA256
93bfe406603df92627f43d9740b201d6b5781da40773105c627ccdd6580e65cc,
119552bytes. This matches the existing candidate with zero changes to any
existing nodes/properties/reservations, and four new __symbols__ entries.
Full proof and pinned source provenance:
results/fp6-selfie-source-review/{source-manifest.json,dtb-equivalence/}.
The original board DTS copyright/license remains its upstream BSD-3-Clause;
the front additions are the separately reviewed experimental board source.
This file is DTS source, not a decoded dump of any live/private device tree.

repack-new-kernel.py is a byte-identical, hash-asserted copy of the existing
kernel-build/repack-new-kernel.py (SHA256062f49e196504c288f0d43bbfbe6cc7d15356e3096b8e327ae67f91fd936da62).
It writes only a new regular Android v2 image file and keeps the original
verified template header/load addresses. Its selected Image.gz and initrd are
unchanged; only the separate DTB input is replaced by the source-built file.

The separate trial target overrides ONLY the Android boot image output.
It never selects this DTB for other targets, recompiles the kernel, changes
modules/vermagic, loads the sensor module, starts capture, or writes a phone.
Strict-ID physical probing passed under this DTB according to the parent;
streaming/image quality is not accepted or claimed by this source package.

Original board source notice retained here for the preprocessed derivative:
SPDX-License-Identifier: BSD-3-Clause
Copyright (c) 2025, Luca Weiss <luca.weiss@fairphone.com>

The isolated boot output explicitly adds one Mobile NixOS generation selector
for its own system.build.toplevel. This chooses the matching stage2 ahead of
any older saved profile. The target asserts that the original kernelParams
contain no such selector, so it cannot silently create duplicate generation
arguments. Kernel Image/modules and boot.kernelParams remain unchanged.
