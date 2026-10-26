#!/usr/bin/env python3
"""Build an FP6 RAM-boot RECOVERY image by changing ONLY the cmdline of an
already physically accepted Android v2 boot image.

Local file operation only: never touches the phone, fastboot or block devices.
Refuses to overwrite. Verifies that every byte except the two cmdline fields
and the SHA1 id field is identical to the source, that the result still pins
the same mobile-nixos.generation, and records SHA256s.

Recovery = same kernel/DTB/initrd/generation as a known-good boot, but booted
to multi-user.target (no compositor, SSH + ADB + NCM up) with fp6.recovery=1
as a marker. Use it with tools/fastboot-ram.py ("fastboot boot", RAM only).
NEVER "fastboot flash".

Example (the default source is the accepted kernel4/y2959 image 46bc4f54...):
  tools/feature-push/misc-make-recovery-image.py \
      results/feature-push-20261024/misc/recovery-y2959-multiuser.img
"""
import argparse
import hashlib
import json
import pathlib
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_SOURCE = ROOT / ".macmini-builder/native-final/phosh-selfie-rdi16/svvbg0n7vgk7lmn5glrrxzr4phnm1v2w-fp6-nixos-selfie-dphy-kernel4-trial.img"
DEFAULT_SHA = "46bc4f54181ab84da7781a9f5e7688a5b5d1df7f468be48e91197e9ae940af00"
RECOVERY_ARGS = ["systemd.unit=multi-user.target", "systemd.show_status=1", "fp6.recovery=1"]


def read_cmdline(img):
    return (img[64:576] + img[608:1632]).split(b"\0", 1)[0].decode()


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("output", type=pathlib.Path)
    p.add_argument("--source", type=pathlib.Path, default=DEFAULT_SOURCE)
    p.add_argument("--source-sha256", default=DEFAULT_SHA)
    p.add_argument("--extra", action="append", default=[], help="additional cmdline token (repeatable)")
    a = p.parse_args()
    if a.output.exists():
        sys.exit("refusing to overwrite " + str(a.output))
    if str(a.output.resolve()).startswith("/dev/"):
        sys.exit("regular files only")
    src = a.source.read_bytes()
    if hashlib.sha256(src).hexdigest() != a.source_sha256:
        sys.exit("source SHA256 mismatch")
    if src[:8] != b"ANDROID!":
        sys.exit("not an Android boot image")
    ksz, kaddr, rsz, raddr, ssz, _, _, page, version, _ = struct.unpack_from("<10I", src, 8)
    if version != 2 or page != 4096 or ssz != 0:
        sys.exit("expected header v2, page 4096, no second stage")
    dtb_size = struct.unpack_from("<I", src, 1648)[0]
    old = read_cmdline(src)
    gens = [t for t in old.split() if t.startswith("mobile-nixos.generation=")]
    if len(gens) != 1:
        sys.exit("source must pin exactly one mobile-nixos.generation")
    tokens = [t for t in old.split() if not t.startswith("systemd.unit=") and t not in ("systemd.show_status=1", "fp6.recovery=1")]
    new = " ".join(tokens + RECOVERY_ARGS + a.extra)
    nb = new.encode()
    if len(nb) >= 1536 or b"\0" in nb:
        sys.exit("cmdline too long")
    out = bytearray(src)
    out[64:576] = nb[:512].ljust(512, b"\0")
    out[608:1632] = nb[512:].ljust(1024, b"\0")
    # id = SHA1 over kernel, ramdisk, second, (recovery dtbo), dtb with sizes, as in repack-new-kernel.py
    def part(off, size):
        return bytes(src[off:off + size])
    koff = page
    roff = koff + (ksz + page - 1) // page * page
    doff = roff + (rsz + page - 1) // page * page
    h = hashlib.sha1()
    for blob in (part(koff, ksz), part(roff, rsz), b"", b"", part(doff, dtb_size)):
        h.update(blob)
        h.update(struct.pack("<I", len(blob)))
    out[576:608] = h.digest() + bytes(12)
    changed = [i for i in range(len(src)) if src[i] != out[i]]
    allowed = set(range(64, 576)) | set(range(608, 1632)) | set(range(576, 608))
    if any(i not in allowed for i in changed):
        sys.exit("internal error: bytes outside cmdline/id changed")
    if read_cmdline(out).split().count(gens[0]) != 1:
        sys.exit("generation pin lost")
    with a.output.open("xb") as f:
        f.write(out)
    rec = {
        "purpose": "FP6 RAM-boot recovery image (fastboot boot only; never flash)",
        "source": str(a.source), "source_sha256": a.source_sha256,
        "output": str(a.output), "sha256": hashlib.sha256(out).hexdigest(), "size": len(out),
        "generation": gens[0].split("=", 1)[1],
        "old_cmdline": old, "new_cmdline": new,
        "changed_byte_ranges": "cmdline [64,576) [608,1632) and id [576,608) only",
        "kernel_initrd_dtb_identical": True,
    }
    a.output.with_suffix(".json").write_text(json.dumps(rec, indent=1) + "\n")
    print(json.dumps(rec, indent=1))


if __name__ == "__main__":
    main()
