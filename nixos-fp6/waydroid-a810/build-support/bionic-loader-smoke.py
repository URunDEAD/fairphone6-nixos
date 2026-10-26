#!/usr/bin/env python3
"""Run Android's native ARM64 loader only; never calls graphics entry points."""
from pathlib import Path
import argparse,hashlib,json,os,shutil,subprocess
r=Path(__file__).resolve().parent
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--stage',type=Path,default=r/'android-build/overlay-vendor/lib64')
p.add_argument('--runtime',type=Path,default=r/'android-runtime')
p.add_argument('--toolchain',type=Path,default=r/'android-build/toolchain.json')
p.add_argument('--work-directory',type=Path,default=r/'android-build')
a=p.parse_args();a.work_directory.mkdir(parents=True,exist_ok=True)
cfg=json.loads(a.toolchain.read_text())
exe=a.work_directory/'bionic-dlopen-probe'
subprocess.run([*cfg['c'],*cfg['c_link_args'],r/'bionic-dlopen-probe.c','-ldl','-o',exe],check=True)
# Never change the saved runtime evidence's permissions or contents.
linker=a.work_directory/'linker64'
shutil.copyfile(a.runtime/'linker64',linker);linker.chmod(0o755)
s=a.stage
libs=[s/p for p in ['libgallium_dri.so','libgbm_mesa.so','dri_gbm.so','egl/libEGL_mesa.so','egl/libGLESv1_CM_mesa.so','egl/libGLESv2_mesa.so','hw/vulkan.freedreno.so']]
env=dict(os.environ,LD_LIBRARY_PATH=':'.join(map(str,[a.runtime,s,s/'egl',s/'hw'])))
result=subprocess.run([linker,exe,*libs],env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=10)
(a.work_directory/'bionic-dlopen-output.txt').write_text(result.stdout)
print(result.stdout,end='')
if result.returncode or result.stdout.count('LOADED ')!=7:raise SystemExit(result.returncode or 1)
(a.work_directory/'bionic-dlopen-validation.json').write_text(json.dumps({'status':'PASS','exit_status':result.returncode,'loaded':7,'graphics_API_calls':0,'library_sha256':{str(f.relative_to(s)):hashlib.sha256(f.read_bytes()).hexdigest() for f in libs},'toolchain_sha256':hashlib.sha256(a.toolchain.read_bytes()).hexdigest(),'runtime_manifest_sha256':hashlib.sha256((a.runtime/'provenance.json').read_bytes()).hexdigest(),'environment':'native aarch64 Linux VM; actual Android13 system-image linker and runtime libraries; no GPU device; generated Android namespace config absent','scope':'Proves Bionic relocations and library constructors only, not Android namespace permissions, buffer ABI or rendering.'},indent=2)+'\n')
