#!/usr/bin/env python3
"""Memory pressure for the low-memory-killer test: allocate incompressible
memory in 128 MiB steps until fp6-app-ready has closed STOP_AFTER apps, or
MemAvailable < 3 %, or LIMIT_MB, or TIMEOUT s. This process asks to be the
first kernel/earlyoom victim (oom_score_adj 1000) and frees everything on exit."""
import os, random, subprocess, sys, time
open('/proc/self/oom_score_adj', 'w').write('1000')
limit = int(os.environ.get('LIMIT_MB', '10000'))
stop_after = int(os.environ.get('STOP_AFTER', '2'))
timeout = float(os.environ.get('TIMEOUT', '150'))
since = time.strftime('%H:%M:%S')
def meminfo():
    m = {}
    for line in open('/proc/meminfo'):
        k, v = line.split(':'); m[k] = int(v.split()[0])
    return m
def kills():
    out = subprocess.run(['journalctl', '--user', '-u', 'fp6-app-ready', '--since', since, '-o', 'cat', '--no-pager'],
                         capture_output=True, text=True).stdout
    return [l for l in out.splitlines() if ': closed ' in l]
blocks, t0 = [], time.time()
while True:
    m = meminfo()
    avail = m['MemAvailable'] / m['MemTotal']
    k = kills()
    print(f"{time.time()-t0:6.1f}s alloc={len(blocks)*128}MiB avail={m['MemAvailable']>>10}MiB ({avail:.1%}) swapfree={m['SwapFree']>>10}MiB kills={len(k)}", flush=True)
    if len(k) >= stop_after or avail < 0.03 or len(blocks) * 128 >= limit or time.time() - t0 > timeout:
        break
    blocks.append(random.randbytes(128 << 20))
    time.sleep(0.3)
for l in kills():
    print('KILL', l, flush=True)
