"""Create an FP6 RAM-test image with an explicitly supplied NEW kernel and DTB.

Writes a regular output file only. Never accesses phone/block devices.
Preserves the proven Android v2 load addresses and bootloader metadata.
"""
import argparse
import gzip
import hashlib
from pathlib import Path
import struct

p = argparse.ArgumentParser()
for name in ('template', 'kernel', 'initrd', 'dtb', 'output'):
    p.add_argument(name, type=Path)
p.add_argument('--cmdline', required=True)
a = p.parse_args()
assert not a.output.exists(), 'Refusing to overwrite an existing output'
assert not str(a.output.resolve()).startswith('/dev/'), 'Regular output files only'
template = a.template.read_bytes()
assert hashlib.sha256(template).hexdigest() == '98ed20373d229a5ebac482f81f7f601150137db3d9f5220ef8b6d6feab781cf8'
assert template[:8] == b'ANDROID!'
kernel_size, kernel_address, initrd_size, initrd_address, second_size, _, _, page, version, _ = struct.unpack_from('<10I', template, 8)
assert version == 2 and page == 4096 and second_size == 0
assert struct.unpack_from('<I', template, 1632)[0] == 0
dtb_address = struct.unpack_from('<Q', template, 1652)[0]
kernel, initrd, dtb = (x.read_bytes() for x in (a.kernel, a.initrd, a.dtb))
assert kernel[:2] == b'\x1f\x8b', 'Use Image.gz, matching the proven boot format'
image = gzip.decompress(kernel)
assert image[56:60] == b'ARMd', 'Not an AArch64 Linux Image'
assert dtb[:4] == b'\xd0\x0d\xfe\xed'
assert struct.unpack_from('>I', dtb, 4)[0] == len(dtb), 'DTB has trailing data or is truncated'
assert kernel_address + len(kernel) <= initrd_address, 'Compressed kernel overlaps initrd load address'
assert initrd_address + len(initrd) <= dtb_address, 'Initrd overlaps DTB load address'
cmdline = a.cmdline.encode()
assert len(cmdline) < 1536 and b'\0' not in cmdline
header = bytearray(template[:page])
struct.pack_into('<I', header, 8, len(kernel))
struct.pack_into('<I', header, 16, len(initrd))
struct.pack_into('<I', header, 1648, len(dtb))
header[64:576] = cmdline[:512].ljust(512, b'\0')
header[608:1632] = cmdline[512:].ljust(1024, b'\0')
h = hashlib.sha1()
for part in (kernel, initrd, b'', b'', dtb):
    h.update(part)
    h.update(struct.pack('<I', len(part)))
header[576:608] = h.digest() + bytes(12)
output = bytes(header)
for part in (kernel, initrd, dtb):
    output += part.ljust((len(part) + page - 1) // page * page, b'\0')
assert len(output) < 96 * 1024 * 1024
with a.output.open('xb') as f:
    f.write(output)
print(f'{hashlib.sha256(output).hexdigest()}  {a.output} ({len(output)} bytes)')
