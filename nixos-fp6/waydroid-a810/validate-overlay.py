#!/usr/bin/env python3
"""Validate seven Android libraries without executing them or rewriting ELF."""
import argparse, hashlib, json, re, subprocess
from pathlib import Path
REQUIRED = {
 'libgallium_dri.so': ('libgallium_dri.so', ['_mesa_glapi_tls_Dispatch']),
 'libgbm_mesa.so': ('libgbm_mesa.so', ['gbm_create_device','gbm_bo_create_with_modifiers2']),
 'dri_gbm.so': ('dri_gbm.so', ['gbmint_get_backend']),
 'egl/libEGL_mesa.so': ('libEGL.so', ['eglInitialize','eglGetDisplay','eglGetProcAddress']),
 'egl/libGLESv1_CM_mesa.so': ('libGLESv1_CM.so', ['glClear','glDrawArrays']),
 'egl/libGLESv2_mesa.so': ('libGLESv2.so', ['glClear','glDrawArrays','glCreateShader']),
 'hw/vulkan.freedreno.so': ('libvulkan_freedreno.so', ['HMI']),
}
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('tree',type=Path)
p.add_argument('--expected',type=Path)
p.add_argument('--consumer-imports',type=Path,required=True)
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
expected=json.loads(a.expected.read_text()) if a.expected else None
assert expected is None or set(expected)==set(REQUIRED), 'Unexpected artifact manifest'
assert set(str(f.relative_to(a.tree)) for f in a.tree.rglob('*') if not f.is_dir())==set(REQUIRED), 'Tree must contain exactly seven files'
report={}
for rel,(soname,exports) in REQUIRED.items():
 f=a.tree/rel
 assert f.is_file() and not f.is_symlink(), (rel,'must be a regular file')
 digest=hashlib.sha256(f.read_bytes()).hexdigest()
 if expected: assert digest==expected[rel]['sha256'],(rel,'hash mismatch')
 details=subprocess.check_output(['readelf','-h','-d','--wide',f],text=True)
 assert 'AArch64' in details and 'ELF64' in details and 'DYN (Shared object file)' in details,rel
 assert re.findall(r'\(SONAME\).*?\[(.*?)\]',details)==[soname],(rel,'SONAME')
 assert 'RPATH' not in details and 'RUNPATH' not in details,(rel,'loader search path')
 needed=re.findall(r'\(NEEDED\).*?\[(.*?)\]',details)
 assert not any(n.endswith('.so.6') or n in ['libc++_shared.so','libstdc++.so.6'] for n in needed),(rel,'host/shared C++ dependency')
 syms=subprocess.check_output(['readelf','--dyn-syms','--wide',f],text=True)
 defs={line.split()[-1].split('@')[0] for line in syms.splitlines() if len(line.split())>=8 and ' UND ' not in line and (' GLOBAL ' in line or ' WEAK ' in line)}
 assert set(exports)<=defs,(rel,'missing public entry points',set(exports)-defs)
 if rel=='libgbm_mesa.so':
  for consumer,imports in json.loads(a.consumer_imports.read_text()).items():
   assert set(imports)<=defs,(consumer,'missing GBM imports',set(imports)-defs)
 report[rel]={'sha256':digest,'soname':soname,'needed':needed,'required_exports':exports}
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text(json.dumps(report,indent=2)+'\n')
print('PASS: exact seven Android AArch64 libraries, hashes/SONAMEs/exports/GBM imports; no host ABI or RPATH')
