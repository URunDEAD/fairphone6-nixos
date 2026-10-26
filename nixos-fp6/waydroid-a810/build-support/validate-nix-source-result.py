#!/usr/bin/env python3
"""Run bounded static ABI and native Bionic loader checks on a Nix source result.
No Android/container/GPU or phone operation. Requires the previously verified
public Android13 runtime/consumer files already available locally.
"""
import argparse,hashlib,json,subprocess,sys
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--package',type=Path,required=True)
p.add_argument('--runtime',type=Path,required=True)
p.add_argument('--consumers',type=Path,required=True)
p.add_argument('--output',type=Path,required=True)
p.add_argument('--toolchain',type=Path,help='Source-build audit JSON when checking its runtime subset')
a=p.parse_args();here=Path(__file__).resolve().parent
package=a.package.resolve(strict=True)
if not str(package).startswith('/nix/store/'):raise SystemExit('Expected an immutable Nix store package')
a.output.mkdir(parents=True,exist_ok=True)
manifest=json.loads((package/'share/fp6-waydroid-a810/artifacts.json').read_text())
assert len(manifest)==7
for rel,record in manifest.items():
 path=package/'lib64'/rel
 assert path.resolve().is_relative_to(package/'lib64') and path.is_file() and not path.is_symlink()
 assert hashlib.sha256(path.read_bytes()).hexdigest()==record['sha256']
subprocess.run([sys.executable,here/'validate-android-abi.py','--stage',package/'lib64','--runtime',a.runtime,'--consumers',a.consumers,'--output',a.output/'abi-validation.json'],check=True,stdout=(a.output/'abi-validation-output.txt').open('w'))
subprocess.run([sys.executable,here/'bionic-loader-smoke.py','--stage',package/'lib64','--runtime',a.runtime,'--toolchain',a.toolchain or package/'share/fp6-waydroid-a810/toolchain.json','--work-directory',a.output],check=True,timeout=30)
abi=json.loads((a.output/'abi-validation.json').read_text());loader=json.loads((a.output/'bionic-dlopen-validation.json').read_text())
assert abi['status']=='PASS' and loader['status']=='PASS'
summary={'status':'PASS','package':str(package),'libraries':7,'package_manifest_hashes_verified':True,'runtime_files_hash_verified':abi['runtime_files_hash_verified'],'strong_symbol_version_closure':'PASS','actual_baseline_gbm_imports':'PASS','bionic_RTLD_NOW_loaded':loader['loaded'],'graphics_API_calls':0,'phone_actions':0,'android_containers_started':0,'hardware_acceptance':False}
(a.output/'source-validation-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
