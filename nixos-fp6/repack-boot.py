"""Replace only initrd/cmdline in the verified FP6 Android v2 boot format."""
import argparse
import hashlib
from pathlib import Path
import struct

parser = argparse.ArgumentParser()
parser.add_argument("template", type=Path)
parser.add_argument("initrd", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--cmdline", default="console=tty0 loglevel=7")
args = parser.parse_args()
original = args.template.read_bytes()
assert original[:8] == b"ANDROID!"
kernel_size, _, old_initrd_size, _, second_size, _, _, page, version, _ = struct.unpack_from("<10I", original, 8)
assert version == 2 and page == 4096 and second_size == 0
assert struct.unpack_from("<I", original, 1632)[0] == 0
dtb_size = struct.unpack_from("<I", original, 1648)[0]
align = lambda n: (n + page - 1) // page * page
kernel = original[page:page + kernel_size]
old_initrd_start = page + align(kernel_size)
old_initrd = original[old_initrd_start:old_initrd_start + old_initrd_size]
dtb_start = old_initrd_start + align(old_initrd_size)
dtb = original[dtb_start:dtb_start + dtb_size]
assert len(dtb) == dtb_size

def image_id(parts):
    h = hashlib.sha1()
    for part in parts:
        h.update(part)
        h.update(struct.pack("<I", len(part)))
    return h.digest() + bytes(12)

assert original[576:608] == image_id([kernel, old_initrd, b"", b"", dtb])
initrd = args.initrd.read_bytes()
ramdisk_address = struct.unpack_from("<I", original, 20)[0]
dtb_address = struct.unpack_from("<Q", original, 1652)[0]
assert ramdisk_address + len(initrd) <= dtb_address, "Initrd would overlap the preserved DTB load address"
cmdline = args.cmdline.encode()
assert len(cmdline) < 1536 and b"\0" not in cmdline
header = bytearray(original[:page])
struct.pack_into("<I", header, 16, len(initrd))
header[64:576] = cmdline[:512].ljust(512, b"\0")
header[608:1632] = cmdline[512:].ljust(1024, b"\0")
header[576:608] = image_id([kernel, initrd, b"", b"", dtb])
output = bytes(header)
for part in (kernel, initrd, dtb):
    output += part.ljust(align(len(part)), b"\0")
assert len(output) < 96 * 1024 * 1024
args.output.write_bytes(output)
print(f"{hashlib.sha256(output).hexdigest()}  {args.output} ({len(output)} bytes)")
