#!/usr/bin/env python3
"""Static Android runtime ABI validation; no target binary execution."""
from pathlib import Path
import argparse,hashlib,json,re,subprocess
ROOT=Path(__file__).resolve().parent
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--stage',type=Path,default=ROOT/'android-build/overlay-vendor/lib64')
parser.add_argument('--runtime',type=Path,default=ROOT/'android-runtime')
parser.add_argument('--consumers',type=Path,default=ROOT/'phone-vendor-consumers')
parser.add_argument('--output',type=Path,default=ROOT/'android-build/abi-validation.json')
args=parser.parse_args()
stage=args.stage
runtime=args.runtime
runtime_provenance=json.loads((runtime/'provenance.json').read_text())
assert runtime_provenance['system_image_recorded_sha256']=='48af56f69d8f3f415e2278f39f6e4c3e26dde2d33902d0585f79a5872899b61d','Unexpected Android system baseline'
for name,record in runtime_provenance['files'].items():
 assert Path(name).name==name,'Invalid runtime manifest path'
 assert hashlib.sha256((runtime/name).read_bytes()).hexdigest()==record['sha256'],('Changed runtime library',name)
assert {f.name for f in runtime.iterdir()}==set(runtime_provenance['files'])|{'provenance.json'},'Unexpected runtime files'
REQUIRED={
'libgallium_dri.so':['_mesa_glapi_tls_Dispatch'],
'libgbm_mesa.so':['gbm_create_device','gbm_bo_create_with_modifiers2'],
'dri_gbm.so':['gbmint_get_backend'],
'egl/libEGL_mesa.so':['eglGetDisplay','eglInitialize','eglGetProcAddress'],
'egl/libGLESv1_CM_mesa.so':['glClear','glDrawArrays'],
'egl/libGLESv2_mesa.so':['glClear','glDrawArrays','glCreateShader'],
'hw/vulkan.freedreno.so':['HMI'],
}
def inspect(path):
 dyn=subprocess.check_output(['readelf','-d','--wide',path],text=True)
 syms=subprocess.check_output(['readelf','--dyn-syms','--wide',path],text=True)
 defs=set();undef=set();weak=set()
 for line in syms.splitlines():
  # GNU readelf prints Android IFUNC type as the multiword '<OS specific>: 10'.
  m=re.match(r'^\s*\d+:\s+\S+\s+\d+\s+.+?\s+(GLOBAL|WEAK)\s+(DEFAULT|PROTECTED|HIDDEN)\s+(\S+)\s+(\S+)',line)
  if not m:continue
  bind,vis,ndx,name=m.groups();n=name.replace('@@','@')
  if ndx=='UND':(weak if bind=='WEAK' else undef).add(n)
  elif vis in ('DEFAULT','PROTECTED'):
   defs.add(n)
   # Default version is also available to unversioned imports.
   if '@@' in name:defs.add(n.split('@')[0])
 return {'path':str(path),'needed':re.findall(r'\(NEEDED\).*?\[(.*?)\]',dyn),
         'soname':re.findall(r'\(SONAME\).*?\[(.*?)\]',dyn),'defs':defs,'undef':undef,'weak':weak}
alllibs={}
for p in list(runtime.glob('*.so'))+[runtime/'linker64']:
 x=inspect(p);alllibs[(x['soname']or[p.name])[0]]=x
new={}
for rel,req in REQUIRED.items():
 p=stage/rel;x=inspect(p)
 assert len(x['soname'])==1,(rel,'missing SONAME')
 assert set(req)<=x['defs'],(rel,'missing required exports',set(req)-x['defs'])
 alllibs[x['soname'][0]]=x;new[rel]=x
report={};errors=[]
for rel,x in new.items():
 visited=set();todo=x['needed'][:];providers=set(x['defs'])
 while todo:
  soname=todo.pop()
  if soname in visited:continue
  visited.add(soname)
  if soname not in alllibs:errors.append((rel,'missing dependency',soname));continue
  d=alllibs[soname];providers|=d['defs'];todo+=d['needed']
 missing=x['undef']-providers
 if missing:errors.append((rel,'unresolved strong symbols',sorted(missing)))
 report[rel]={'sha256':hashlib.sha256(Path(x['path']).read_bytes()).hexdigest(),
  'soname':x['soname'][0],'needed':x['needed'],'dependency_closure':sorted(visited),
  'strong_imports':len(x['undef']),'optional_weak_imports':sorted(x['weak']),
  'unresolved_strong_imports':sorted(missing),'required_exports':REQUIRED[rel]}
consumers={}
for p in args.consumers.glob('*.so'):
 x=inspect(p)
 required={n for n in x['undef'] if n.startswith('gbm_')}
 missing=required-new['libgbm_mesa.so']['defs']
 consumers[p.name]={'gbm_imports':sorted(required),'missing':sorted(missing)}
 if missing:errors.append((p.name,'missing actual-phone GBM imports',sorted(missing)))
value={'status':'FAIL' if errors else 'PASS','errors':errors,'libraries':report,
 'actual_phone_vendor_consumers':consumers,
 'runtime_system_sha256':runtime_provenance['system_image_recorded_sha256'],
 'runtime_files_hash_verified':len(runtime_provenance['files']),
 'scope':'Static actual Android13 image symbol/version closure and exported entry points. Does not prove namespace accessibility, buffer ABI, or GPU rendering.'}
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps(value,indent=2)+'\n')
print(json.dumps(value,indent=2))
raise SystemExit(bool(errors))
