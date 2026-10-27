#!/usr/bin/env python3
"""Print the APM GRAPH_OPEN params of an acdbq dump (sub-graphs, containers,
modules, ports, connections, control links, VCPM config)."""
import struct,sys
def u(b,o,n=1): return struct.unpack_from('<%dI'%n,b,o)
NAMES={0x08001001:'SUB_GRAPH_CONFIG',0x08001000:'CONTAINER_CONFIG',0x08001002:'MODULES_LIST',0x08001003:'MODULE_PROP',0x08001004:'MODULE_CONN',0x08001061:'MODULE_CTRL_LINK_CFG'}
def props(b,o,n):
    out=[]
    for _ in range(n):
        pid,psz=u(b,o,2); pl=b[o+8:o+8+psz]; out.append((pid,[x for x in struct.unpack('<%dI'%(psz//4),pl)])); o+=8+psz
    return out,o
def parse(b,tag):
    o=0
    while o+16<=len(b):
        iid,pid,psz,err=u(b,o,4); p=b[o+16:o+16+psz]
        print('%s param iid=%#x %s(%#x) size=%d'%(tag,iid,NAMES.get(pid,'?'),pid,psz))
        q=0
        if pid==0x08001001:
            n,=u(p,0); q=4
            for _ in range(n):
                sg,np_=u(p,q,2); pr,q=props(p,q+8,np_); print('  SG %#x'%sg,[(hex(a),[hex(x) for x in c]) for a,c in pr])
        elif pid==0x08001000:
            n,=u(p,0); q=4
            for _ in range(n):
                c,np_=u(p,q,2); pr,q=props(p,q+8,np_); print('  CONT %#x'%c,[(hex(a),[hex(x) for x in cc]) for a,cc in pr])
        elif pid==0x08001002:
            n,=u(p,0); q=4
            for _ in range(n):
                sg,c,nm=u(p,q,3); q+=12
                for _ in range(nm):
                    mid,iid2=u(p,q,2); q+=8; print('  MOD sg=%#x cont=%#x mid=%#010x iid=%#x'%(sg,c,mid,iid2))
        elif pid==0x08001003:
            n,=u(p,0); q=4
            for _ in range(n):
                i2,np_=u(p,q,2); pr,q=props(p,q+8,np_); print('  MPROP iid=%#x'%i2,[(hex(a),c) for a,c in pr])
        elif pid in (0x08001004,):
            n,=u(p,0); q=4
            for _ in range(n):
                s,sp,d,dp=u(p,q,4); q+=16; print('  CONN %#x:%d -> %#x:%d'%(s,sp,d,dp))
        elif pid==0x08001061:
            n,=u(p,0); q=4
            for _ in range(n):
                s,sp,d,dp,np_=u(p,q,5); pr,q=props(p,q+20,np_); print('  CTRL %#x:%d <-> %#x:%d'%(s,sp,d,dp),[(hex(a),[hex(x) for x in c]) for a,c in pr])
        else:
            print('  raw',[hex(x) for x in struct.unpack('<%dI'%(psz//4),p)][:40])
        o+=16+((psz+7)&~7)
for f in sys.argv[1:]:
    print('=====',f); parse(open(f,'rb').read(),f.split('/')[-1])
