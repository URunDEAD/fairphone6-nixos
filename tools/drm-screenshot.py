#!/usr/bin/env python3
"""Read the active linear DRM framebuffer and emit a PNG to stdout."""
import ctypes as C
import fcntl
import mmap
import os
import struct
import sys
import zlib

class Resources(C.Structure):
    _fields_ = [('count_fbs', C.c_int), ('fbs', C.POINTER(C.c_uint32)),
                ('count_crtcs', C.c_int), ('crtcs', C.POINTER(C.c_uint32)),
                ('count_connectors', C.c_int), ('connectors', C.POINTER(C.c_uint32)),
                ('count_encoders', C.c_int), ('encoders', C.POINTER(C.c_uint32)),
                ('min_width', C.c_uint32), ('max_width', C.c_uint32),
                ('min_height', C.c_uint32), ('max_height', C.c_uint32)]

class FB(C.Structure):
    _fields_ = [('fb_id', C.c_uint32), ('width', C.c_uint32), ('height', C.c_uint32),
                ('pixel_format', C.c_uint32), ('modifier', C.c_uint64), ('flags', C.c_uint32),
                ('handles', C.c_uint32 * 4), ('pitches', C.c_uint32 * 4),
                ('offsets', C.c_uint32 * 4)]

d = C.CDLL('/nix/store/5qsszzg735l7qd2bc07601nq0hkdzjda-libdrm-2.4.134/lib/libdrm.so.2', use_errno=True)
d.drmModeGetResources.argtypes = [C.c_int]
d.drmModeGetResources.restype = C.POINTER(Resources)
d.drmModeGetCrtc.argtypes = [C.c_int, C.c_uint32]
d.drmModeGetCrtc.restype = C.POINTER(C.c_uint32)
d.drmModeGetFB2.argtypes = [C.c_int, C.c_uint32]
d.drmModeGetFB2.restype = C.POINTER(FB)
d.drmPrimeHandleToFD.argtypes = [C.c_int, C.c_uint32, C.c_uint32, C.POINTER(C.c_int)]
fd = os.open('/dev/dri/card0', os.O_RDWR | os.O_CLOEXEC)
r = d.drmModeGetResources(fd).contents
for i in range(r.count_crtcs):
    crtc = d.drmModeGetCrtc(fd, r.crtcs[i])
    if crtc and crtc[1]:
        fb = d.drmModeGetFB2(fd, crtc[1]).contents
        break
else:
    raise RuntimeError('No active framebuffer')
print(f'{fb.width}x{fb.height} format={fb.pixel_format:08x} modifier={fb.modifier:x} '
      f'pitch={fb.pitches[0]} handle={fb.handles[0]}', file=sys.stderr)
assert fb.modifier == 0, 'Framebuffer is tiled/compressed'
assert fb.pixel_format in (0x34325258, 0x34325241, 0x34324258, 0x34324241), 'Unsupported pixel format'
prime_fd = C.c_int()
assert d.drmPrimeHandleToFD(fd, fb.handles[0], os.O_CLOEXEC | os.O_RDWR, C.byref(prime_fd)) == 0, C.get_errno()
length = fb.offsets[0] + fb.pitches[0] * fb.height
fcntl.ioctl(prime_fd.value, 0x40086200, struct.pack('Q', 1))  # DMA_BUF_SYNC_START | READ
with mmap.mmap(prime_fd.value, length, flags=mmap.MAP_SHARED, prot=mmap.PROT_READ) as pixels:
    raw = bytearray()
    rgb_order = fb.pixel_format in (0x34324258, 0x34324241)
    for y in range(fb.height):
        start = fb.offsets[0] + y * fb.pitches[0]
        row = pixels[start:start + fb.width * 4]
        out = bytearray(fb.width * 3)
        out[0::3] = row[(0 if rgb_order else 2)::4]
        out[1::3] = row[1::4]
        out[2::3] = row[(2 if rgb_order else 0)::4]
        raw.extend(b'\0' + out)
fcntl.ioctl(prime_fd.value, 0x40086200, struct.pack('Q', 5))  # DMA_BUF_SYNC_END | READ

def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))

sys.stdout.buffer.write(b'\x89PNG\r\n\x1a\n' +
    chunk(b'IHDR', struct.pack('>IIBBBBB', fb.width, fb.height, 8, 2, 0, 0, 0)) +
    chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))
os.close(prime_fd.value)
os.close(fd)
