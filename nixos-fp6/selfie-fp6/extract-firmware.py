"""Decode only exact hash-pinned public FP6 register data; never execute a blob.

Derived from the reviewed extract-fp6-static.py/make-candidate-firmware.py.
This reproduces the3976-byte candidate payload, not private calibration data.
"""
from pathlib import Path
import hashlib,struct,sys
b=Path(sys.argv[1]).read_bytes()
assert hashlib.sha256(b).hexdigest()=='7a924c8a79d526f0258896a0d048ac85b73fa989d273511b1e5e3e94f7f5d0ce'
u32=lambda o:struct.unpack_from('<I',b,o)[0]
assert b[:20]==b'QTI Chromatix Header' and u32(28)+4==len(b) and u32(32)==5
assert b[40:].startswith(b'Parameter Parser V5.0.2 (2311081932)\0')
section_offset,section_count=u32(160),u32(164)
assert section_count==4
sections={}
for i in range(section_count):
 kind,off,size=struct.unpack_from('<III',b,section_offset+12*i)
 assert kind not in sections and off+size<=len(b)
 sections[kind]=(off,size)
assert set(sections)=={0,1,2,3}
meta,mlen=sections[3];data,dlen=sections[1]
assert mlen%16==0
nodes=[struct.unpack_from('<4I',b,meta+16*i) for i in range(mlen//16)]
for i,(ident,off,size,ref) in enumerate(nodes):
 assert ident==i+1 and off+size<=dlen and 0<ref<=len(nodes)
 _,roff,rsize,rref=nodes[ref-1]
 assert (off,size)==(roff,rsize)
def node(ident):
 assert 0<ident<=len(nodes)
 _,off,size,_=nodes[ident-1]
 return b[data+off:data+off+size]
def scalar(ident):
 raw=node(ident);assert len(raw)==4
 return struct.unpack('<I',raw)[0]
result=b'FP6KD1\0\1'+struct.pack('>II',258,237)
for ident,count in [(7426,258),(1271,237)]:
 raw=node(ident);assert len(raw)==56*count
 for i in range(count):
  w=struct.unpack_from('<14I',raw,56*i)
  assert w[0]==w[2]==w[4]==w[11]==0 and w[7]==w[12]==1
  assert w[9]==w[10]==2 and w[6]<=0xffff
  assert all(len(node(w[x]))==0 for x in [1,3,5])
  value,delay=scalar(w[8]),scalar(w[13])
  assert value<=0xffff and delay<=10000
  result+=struct.pack('>HHI',w[6],value,delay)
assert len(result)==3976
assert hashlib.sha256(result).hexdigest()=='42e47a8efaa51e1370599a827f5a45603e54220265f5763cfb79197703c20f68'
Path(sys.argv[2]).write_bytes(result)
