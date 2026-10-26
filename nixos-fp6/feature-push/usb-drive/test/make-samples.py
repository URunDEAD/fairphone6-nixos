#!/usr/bin/env python3
"""Small sample images for UI previews (sparse; real headers, no payload)."""
import os, sys, lzma, hashlib

d = sys.argv[1]

def iso(path, size, label, hybrid, uefi):
    with open(path, "wb") as f:
        f.truncate(size)
        def at(off, data):
            f.seek(off); f.write(data)
        p = 16 * 2048
        at(p, b"\x01CD001\x01"); at(p + 40, label.ljust(32).encode())
        b = 17 * 2048
        at(b, b"\x00CD001\x01EL TORITO SPECIFICATION"); at(b + 0x47, (20).to_bytes(4, "little"))
        at(18 * 2048, b"\xffCD001\x01")
        c = 20 * 2048
        at(c, b"\x01\x00"); at(c + 0x1E, b"\x55\xaa"); at(c + 32, b"\x88")
        if uefi:
            at(c + 64, b"\x91\xef\x01\x00"); at(c + 96, b"\x88")
        if hybrid:
            at(446, b"\x80\x00\x00\x00\x17"); at(446 + 12, (100).to_bytes(4, "little")); at(510, b"\x55\xaa")

G = 1 << 30
iso(os.path.join(d, "nixos-minimal-26.05-x86_64-linux.iso"), int(1.6 * G) // 2048 * 2048, "nixos-minimal-26.05-x86_64", True, True)
iso(os.path.join(d, "debian-13.7.0-amd64-netinst.iso"), 790 * (1 << 20), "Debian 13.7.0 amd64 n", True, True)
iso(os.path.join(d, "old-rescue-cd.iso"), 300 * (1 << 20), "RESCUE", False, False)
# raw GPT disk image
with open(os.path.join(d, "raspios-arm64.img"), "wb") as f:
    f.truncate(5 * G)
    f.seek(446 + 4); f.write(b"\xee"); f.seek(446 + 12); f.write((1000).to_bytes(4, "little"))
    f.seek(510); f.write(b"\x55\xaa"); f.seek(512); f.write(b"EFI PART")
# compressed
with open(os.path.join(d, "fedora-coreos.raw.xz"), "wb") as f:
    f.write(lzma.compress(b"\0" * 4096))
# verified-looking sums file for the debian image (wrong on purpose? no: unknown)
open(os.path.join(d, "SHA256SUMS"), "w").write(hashlib.sha256(b"x").hexdigest() + "  debian-13.7.0-amd64-netinst.iso\n")
