#!/usr/bin/env python3
"""Exact fresh trial config gate; writes review evidence without starting it."""
import hashlib,json,sys
from pathlib import Path
r=Path(sys.argv[1]).resolve();m=json.loads((r/'trial.json').read_text());config=(r/'lxc'/r.name/'config').read_text()
assert r.parent==Path('/var/lib/fp6-waydroid-trials') and m['mode'] in ['vm-software','phone-gpu']
assert f'lxc.rootfs.path = dir:{r}/rootfs\n' in config
assert f'lxc.mount.entry = {r}/data data none bind,create=dir 0 0\n' in config
assert 'lxc.net.0.type = empty\n' in config
for line in ['lxc.cgroup2.memory.max = 2147483648','lxc.cgroup2.memory.swap.max = 536870912','lxc.cgroup2.cpu.max = 200000 100000']:assert line+'\n' in config
for forbidden in ['/dev/fb','/dev/video','/home/user','/var/lib/waydroid','waydroid0','/pulse/native']:assert forbidden not in config,forbidden
gpu=[]
if m['mode']=='vm-software':
 for forbidden in ['/dev/dri','/dev/dma_heap']:assert forbidden not in config,forbidden
else:
 gpu=['/dev/dri/renderD128','/dev/dma_heap/system']
 assert m['wayland_socket'],'GPU mode requires explicit owned compositor socket'
 for device in gpu:assert f'lxc.mount.entry = {device} {device[1:]} none bind,create=file 0 0\n' in config
 for line in config.splitlines():
  if line.startswith('lxc.mount.entry = /dev/dri') or line.startswith('lxc.mount.entry = /dev/dma_heap'):
   assert line.split()[2] in gpu,'Unexpected GPU device'
for node in ['/dev/random','/dev/urandom',' dev/kmsg ']:assert node not in config,'Android init must create '+node
for upper in ['upper-system','upper-vendor']:assert ((r/upper).stat().st_mode&0o777)==0o755
for binder in ['binder','vndbinder','hwbinder']:assert f'{r}/binderfs/{binder} dev/{binder}' in config
props=(r/'vendor-overlay/waydroid.prop').read_text()
required=['ro.hardware.egl=swiftshader','ro.hardware.gralloc=default'] if m['mode']=='vm-software' else ['ro.hardware.egl=mesa','ro.hardware.gralloc=gbm','ro.hardware.vulkan=freedreno','gralloc.gbm.device=/dev/dri/renderD128']
for item in required:assert item+'\n' in props
assert not any((r/'data').iterdir()) and (r.stat().st_mode&0o777)==0o700
assert 'android.hardware.media.omx' not in (r/'vendor-overlay/etc/vintf/manifest.xml').read_text()
libs=list((r/'vendor-overlay/lib64').rglob('*.so'));assert len(libs)==7
for f in libs:
 rel=str(f.relative_to(r/'vendor-overlay/lib64'));assert hashlib.sha256(f.read_bytes()).hexdigest()==m['mesa_libraries'][rel]
for name in ['system','vendor']:
 p=r/'images'/(name+'.img');h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
 assert h.hexdigest()==m[name+'_sha256']
report={'status':'PASS','root':str(r),'mode':m['mode'],'memory_max':2147483648,'swap_max':536870912,'cpu_max':'200000 100000','network':'empty namespace','GPU_devices':gpu,'userdata':'new empty private directory','vendor_files':'exactly seven matched libraries + preserved OMX manifest + explicit trial-mode properties','config_sha256':hashlib.sha256(config.encode()).hexdigest(),'system_sha256':m['system_sha256'],'vendor_sha256':m['vendor_sha256'],'wayland_socket':m['wayland_socket'],'automatic_start':False}
(r/'config-review.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));print(config)
