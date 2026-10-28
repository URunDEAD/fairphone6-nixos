#!/usr/bin/env python3
# Check the NT37705 per-rate init tables of a patched panel-novatek-nt37705.c
# against the vendor dtsi (timings @0 120, @1 60, @2 90, @3 30 Hz).
# Usage: python3 -I verify-panel-tables.py <vendor.dtsi> <patched driver.c> <scratch dir>
# (compiles the tables as a host C program in <scratch dir>)
import re, sys, subprocess, json, os, tempfile
dts, csrc, workdir = sys.argv[1], sys.argv[2], sys.argv[3]
c = open(csrc).read()
start = c.index('struct nt37705_rate {'); end = c.index('#define NT37705_HTOTAL')
host = '#include <stdio.h>\n#include <stdbool.h>\ntypedef unsigned char u8;\n#define ARRAY_SIZE(a) (sizeof(a)/sizeof((a)[0]))\n' + c[start:end] + r'''
static void pr(const char *n, const u8 *a, int len){printf("\"%s\":[",n);for(int i=0;i<len;i++)printf("%s%d",i?",":"",a[i]);printf("],");}
int main(void){printf("[");for(unsigned i=0;i<ARRAY_SIZE(nt37705_rates);i++){const struct nt37705_rate*r=&nt37705_rates[i];
printf("{\"hz\":%u,\"n90\":%d,\"skip\":%d,",r->vrefresh,r->native_90hz,r->skip_count);
pr("s1",r->skip_seq1,17);pr("s2",r->skip_seq2,17);pr("s3",r->skip_seq3,17);pr("s4",r->skip_seq4,17);pr("bl",r->skip_blank,17);pr("cy",r->seq_cycle,9);pr("a4",r->seq_a4,5);
printf("\"end\":0}%s",i+1<ARRAY_SIZE(nt37705_rates)?",":"");}printf("]\n");return 0;}
'''
src = os.path.join(workdir, 'tables.c'); exe = os.path.join(workdir, 'tables')
open(src, 'w').write(host)
subprocess.check_call(['cc', '-Wall', '-o', exe, src])
got = json.loads(subprocess.check_output([exe]))
lines = open(dts).read().split('\n')
def seq(a, b):
    out = []
    for l in lines[a-1:b]:
        l = l.split('//')[0].strip()
        if re.match(r'^[0-9A-Fa-f]{2}( [0-9A-Fa-f]{2})+$', l):
            out.append([int(x, 16) for x in l.split()][7:])
    return out
def tables(on):
    t = {}; off = None
    for p in on:
        if p[0] == 0x6f: off = p[1]; continue
        if p[0] == 0xba and off is not None and off >= 0x1c: t.setdefault(off, p)
        off = None
    i = [k for k, p in enumerate(on) if p[0] == 0x91][0]; j = [k for k, p in enumerate(on) if p[0] == 0x11][0]
    return t, on[i+1:j]
# on-command line ranges of timings @0 120, @1 60, @2 90, @3 30 (vendor dtsi)
vendor = {120: seq(68, 285), 60: seq(409, 624), 90: seq(740, 952), 30: seq(1067, 1283)}
keys = {0x1c: 's1', 0x2c: 's2', 0x3c: 's3', 0x4c: 's4', 0x6c: 'bl', 0x7c: 'bl', 0x9c: 'cy', 0xa4: 'a4'}
ok = True
for r in got:
    t, tail = tables(vendor[r['hz']])
    for off, k in keys.items():
        if r[k] != t[off]: ok = False; print(r['hz'], hex(off), 'MISMATCH')
    exp_tail = [[0x2f, 0x01]] if r['n90'] else [[0x2f, 2], [0x5a, 1], [0x2f, 0x30], [0x6d, r['skip']]]
    if exp_tail != tail: ok = False; print(r['hz'], 'tail MISMATCH', tail)
    print(r['hz'], 'Hz: tables + rate commands checked')
print('ALL MATCH' if ok else 'MISMATCH'); sys.exit(0 if ok else 1)
