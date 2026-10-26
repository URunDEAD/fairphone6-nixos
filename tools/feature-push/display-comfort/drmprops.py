#!/usr/bin/env python3
"""Read-only: list CRTC gamma size and CRTC/connector properties (+ blob contents summary)."""
import ctypes as C, os, sys, glob, struct
lib = sorted(glob.glob('/nix/store/*-libdrm-*/lib/libdrm.so.2'))[-1]
d = C.CDLL(lib, use_errno=True)
class Res(C.Structure):
    _fields_=[('count_fbs',C.c_int),('fbs',C.POINTER(C.c_uint32)),('count_crtcs',C.c_int),('crtcs',C.POINTER(C.c_uint32)),
              ('count_connectors',C.c_int),('connectors',C.POINTER(C.c_uint32)),('count_encoders',C.c_int),('encoders',C.POINTER(C.c_uint32)),
              ('a',C.c_uint32),('b',C.c_uint32),('c',C.c_uint32),('e',C.c_uint32)]
class ModeInfo(C.Structure):
    _fields_=[('clock',C.c_uint32)]+[(n,C.c_uint16) for n in 'hdisplay hsync_start hsync_end htotal hskew vdisplay vsync_start vsync_end vtotal vscan'.split()]+[('vrefresh',C.c_uint32),('flags',C.c_uint32),('type',C.c_uint32),('name',C.c_char*32)]
class Crtc(C.Structure):
    _fields_=[('crtc_id',C.c_uint32),('buffer_id',C.c_uint32),('x',C.c_uint32),('y',C.c_uint32),('width',C.c_uint32),('height',C.c_uint32),('mode_valid',C.c_int),('mode',ModeInfo),('gamma_size',C.c_int)]
class Props(C.Structure):
    _fields_=[('count_props',C.c_uint32),('props',C.POINTER(C.c_uint32)),('prop_values',C.POINTER(C.c_uint64))]
class Prop(C.Structure):
    _fields_=[('prop_id',C.c_uint32),('flags',C.c_uint32),('name',C.c_char*32),('count_values',C.c_int),('values',C.POINTER(C.c_uint64)),('count_enums',C.c_int),('enums',C.c_void_p),('count_blobs',C.c_int),('blob_ids',C.POINTER(C.c_uint32))]
class Blob(C.Structure):
    _fields_=[('id',C.c_uint32),('length',C.c_uint32),('data',C.c_void_p)]
d.drmModeGetResources.restype=C.POINTER(Res)
d.drmModeGetCrtc.restype=C.POINTER(Crtc)
d.drmModeObjectGetProperties.restype=C.POINTER(Props); d.drmModeObjectGetProperties.argtypes=[C.c_int,C.c_uint32,C.c_uint32]
d.drmModeGetProperty.restype=C.POINTER(Prop); d.drmModeGetProperty.argtypes=[C.c_int,C.c_uint32]
d.drmModeGetPropertyBlob.restype=C.POINTER(Blob); d.drmModeGetPropertyBlob.argtypes=[C.c_int,C.c_uint32]
d.drmModeCrtcGetGamma.argtypes=[C.c_int,C.c_uint32,C.c_uint32,C.c_void_p,C.c_void_p,C.c_void_p]
fd=os.open('/dev/dri/card0',os.O_RDWR|os.O_CLOEXEC)
d.drmSetClientCap(fd,2,1)  # UNIVERSAL_PLANES
d.drmSetClientCap(fd,3,1)  # ATOMIC (to see atomic props)
r=d.drmModeGetResources(fd).contents
for i in range(r.count_crtcs):
    cid=r.crtcs[i]; c=d.drmModeGetCrtc(fd,cid).contents
    print(f"CRTC {cid} fb={c.buffer_id} {c.width}x{c.height} gamma_size={c.gamma_size}")
    if c.gamma_size>0 and c.buffer_id:
        n=c.gamma_size; R=(C.c_uint16*n)(); G=(C.c_uint16*n)(); B=(C.c_uint16*n)()
        rc=d.drmModeCrtcGetGamma(fd,cid,n,R,G,B)
        idx=[0,n//4,n//2,3*n//4,n-1]
        print("  legacy gamma rc",rc,"R",[R[k] for k in idx],"G",[G[k] for k in idx],"B",[B[k] for k in idx])
    p=d.drmModeObjectGetProperties(fd,cid,0xcccccccc).contents
    for j in range(p.count_props):
        pr=d.drmModeGetProperty(fd,p.props[j]).contents; v=p.prop_values[j]
        s=f"  {pr.name.decode()} = {v}"
        if pr.flags & 0x10 and v:  # BLOB
            b=d.drmModeGetPropertyBlob(fd,v)
            if b:
                b=b.contents; s+=f" (blob len {b.length})"
                raw=C.string_at(b.data,b.length)
                if pr.name==b'CTM':
                    m=struct.unpack('<9Q',raw); s+=" "+str([ (x&0x7fffffffffffffff)/2**32*(-1 if x>>63 else 1) for x in m])
                if pr.name==b'GAMMA_LUT':
                    n=b.length//8; e=lambda k: struct.unpack_from('<HHH',raw,k*8)
                    s+=" first/mid/last "+str([e(0),e(n//2),e(n-1)])
        print(s)
