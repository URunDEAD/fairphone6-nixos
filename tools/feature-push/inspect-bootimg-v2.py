#!/usr/bin/env python3
"""Read-only Android boot image v2 inspector (local files only).

Prints header fields, section sizes/sha256, cmdline and the SHA-1 id check,
as JSON. Optionally compares the DTB section with a reference image.
Usage: inspect-bootimg-v2.py IMAGE [--ref REFERENCE_IMAGE] [--kernel Image.gz]
"""
import argparse
import hashlib
import json
import struct
from pathlib import Path


def parse(path):
    data = Path(path).read_bytes()
    assert data[:8] == b'ANDROID!', f'{path}: not an Android boot image'
    (ksz, kaddr, rsz, raddr, ssz, saddr, tags, page, ver, osv) = struct.unpack_from('<10I', data, 8)
    name = data[48:64].rstrip(b'\0').decode(errors='replace')
    cmd = (data[64:576].rstrip(b'\0') + data[608:1632].rstrip(b'\0')).decode()
    sha_id = data[576:608]
    recsz, = struct.unpack_from('<I', data, 1632)
    recoff, = struct.unpack_from('<Q', data, 1636)
    hdrsz, = struct.unpack_from('<I', data, 1644)
    dtbsz, = struct.unpack_from('<I', data, 1648)
    dtbaddr, = struct.unpack_from('<Q', data, 1652)
    rnd = lambda n: (n + page - 1) // page * page
    off = page
    kernel = data[off:off + ksz]; off += rnd(ksz)
    ramdisk = data[off:off + rsz]; off += rnd(rsz)
    second = data[off:off + ssz]; off += rnd(ssz)
    recovery = data[off:off + recsz]; off += rnd(recsz)
    dtb = data[off:off + dtbsz]; off += rnd(dtbsz)
    h = hashlib.sha1()
    for part in (kernel, ramdisk, second, recovery, dtb):
        h.update(part)
        h.update(struct.pack('<I', len(part)))
    sh = lambda b: hashlib.sha256(b).hexdigest()
    return {
        'file': str(path), 'size': len(data), 'sha256': sh(data),
        'header_version': ver, 'page_size': page, 'header_size': hdrsz, 'os_version': osv,
        'name': name, 'kernel_addr': hex(kaddr), 'ramdisk_addr': hex(raddr),
        'second_addr': hex(saddr), 'tags_addr': hex(tags), 'dtb_addr': hex(dtbaddr),
        'kernel_size': ksz, 'kernel_sha256': sh(kernel), 'kernel_gzip': kernel[:2] == b'\x1f\x8b',
        'ramdisk_size': rsz, 'ramdisk_sha256': sh(ramdisk),
        'second_size': ssz, 'recovery_dtbo_size': recsz, 'recovery_dtbo_offset': recoff,
        'dtb_size': dtbsz, 'dtb_sha256': sh(dtb), 'dtb_magic_ok': dtb[:4] == b'\xd0\x0d\xfe\xed',
        'id_sha1_ok': sha_id[:20] == h.digest(), 'used_bytes': off,
        'cmdline': cmd,
    }


p = argparse.ArgumentParser()
p.add_argument('image')
p.add_argument('--ref')
p.add_argument('--kernel', help='expected Image.gz to compare with the kernel section')
a = p.parse_args()
out = {'image': parse(a.image)}
if a.ref:
    ref = parse(a.ref)
    out['ref'] = ref
    keys = ('header_version', 'page_size', 'header_size', 'os_version', 'name', 'kernel_addr',
            'ramdisk_addr', 'second_addr', 'tags_addr', 'dtb_addr', 'second_size', 'recovery_dtbo_size')
    out['layout_same_as_ref'] = {k: out['image'][k] == ref[k] for k in keys}
    out['dtb_same_as_ref'] = out['image']['dtb_sha256'] == ref['dtb_sha256']
if a.kernel:
    out['kernel_matches_file'] = hashlib.sha256(Path(a.kernel).read_bytes()).hexdigest() == out['image']['kernel_sha256']
print(json.dumps(out, indent=2))
