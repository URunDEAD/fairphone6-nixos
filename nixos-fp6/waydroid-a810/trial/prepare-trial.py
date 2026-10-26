#!/usr/bin/env python3
"""Prepare a NEW isolated Waydroid/LXC trial. Does not mount or start anything."""
import argparse,hashlib,json,os,re
from pathlib import Path
import pwd,shutil,subprocess,xml.etree.ElementTree as ET
HERE=Path(__file__).resolve().parent
BUNDLE=HERE.parent/('vm-artifacts' if (HERE.parent/'vm-artifacts').exists() else 'android-build')
SYSTEM_SHA='48af56f69d8f3f415e2278f39f6e4c3e26dde2d33902d0585f79a5872899b61d'
VENDOR_SHA='ad424225175307bbdd098b777962f05d208794bac67b9c1771ee1a2078fd4cc6'
LIBS=['libgallium_dri.so','libgbm_mesa.so','dri_gbm.so','egl/libEGL_mesa.so','egl/libGLESv1_CM_mesa.so','egl/libGLESv2_mesa.so','hw/vulkan.freedreno.so']
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
 return h.hexdigest()
def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--trial-root',type=Path,required=True)
 p.add_argument('--system-image',type=Path,required=True)
 p.add_argument('--vendor-image',type=Path,required=True)
 p.add_argument('--mode',choices=['phone-gpu','vm-software'],required=True)
 p.add_argument('--mesa-tree',type=Path,default=BUNDLE/'overlay-vendor/lib64')
 p.add_argument('--mesa-manifest',type=Path,default=BUNDLE/'artifacts.json')
 p.add_argument('--wayland-socket',type=Path)
 p.add_argument('--host-uid',type=int,required=True)
 p.add_argument('--plan',action='store_true',help='Validate inputs and print plan only')
 a=p.parse_args();r=a.trial_root.absolute()
 if r.parent!=Path('/var/lib/fp6-waydroid-trials') or not re.fullmatch(r'fp6-[A-Za-z0-9_-]{1,50}',r.name) or r.exists() or r.is_symlink():p.error('Use a new /var/lib/fp6-waydroid-trials/fp6-* root')
 if a.host_uid<=0:p.error('host-uid must be the actual non-root compositor user')
 host=pwd.getpwuid(a.host_uid)
 for inp,expected in [(a.system_image,SYSTEM_SHA),(a.vendor_image,VENDOR_SHA)]:
  if not inp.is_file() or inp.is_symlink() or sha(inp)!=expected:p.error(f'Wrong baseline image/hash: {inp}')
 expected_libs=json.loads(a.mesa_manifest.read_text())
 if set(expected_libs)!=set(LIBS):p.error('Mesa manifest must contain exactly the matched seven libraries')
 for rel in LIBS:
  f=a.mesa_tree/rel
  if not f.is_file() or f.is_symlink() or sha(f)!=expected_libs[rel]['sha256']:p.error(f'Wrong Mesa library/hash: {f}')
 manifest=HERE/'baseline/manifest.xml'
 if 'android.hardware.media.omx' in manifest.read_text() or ET.parse(manifest).getroot().tag!='manifest':p.error('Unexpected OMX manifest baseline')
 if a.wayland_socket:
  sock=a.wayland_socket.resolve(strict=True)
  import stat
  if any(c.isspace() for c in str(sock)) or not stat.S_ISSOCK(sock.stat().st_mode) or sock.stat().st_uid!=a.host_uid:p.error('Wayland socket must belong to host-uid and have no path whitespace')
 else:sock=None
 if a.mode=='phone-gpu' and sock is None:p.error('phone-gpu requires explicit compositor Wayland socket')
 meta={'root':str(r),'name':r.name,'mode':a.mode,'system_sha256':SYSTEM_SHA,'vendor_sha256':VENDOR_SHA,'mesa_libraries':{k:expected_libs[k]['sha256'] for k in LIBS},'omx_manifest_sha256':sha(manifest),'wayland_socket':str(sock) if sock else None,'host_uid':a.host_uid,'memory_max_bytes':2147483648,'swap_max_bytes':536870912,'cpu_max':'200000 100000','automatic_start':False,'userdata':'new empty data directory; no existing Android /data read or mounted','network':'new empty network namespace; no bridge or host network'}
 print(json.dumps(meta,indent=2))
 if a.plan:return
 if os.geteuid()!=0:p.error('Preparation requires root for private directory ownership')
 r.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
 if r.parent.is_symlink() or r.parent.resolve()!=r.parent:p.error('Trial parent must be a real directory')
 r.mkdir(mode=0o700)
 for d in ['images','data','logs','vendor-overlay/etc/vintf','rootfs','lower-system','lower-vendor','upper-system','work-system','upper-vendor','work-vendor','binderfs','lxc/'+r.name]:
  (r/d).mkdir(parents=True,exist_ok=True,mode=0o700)
 # Overlay roots inherit upperdir mode. The OUTER trial remains0700; Android's
 # visible / and /vendor must be traversable by its non-root service UIDs.
 for d in ['upper-system','upper-vendor']:(r/d).chmod(0o755)
 for inp,name in [(a.system_image,'system.img'),(a.vendor_image,'vendor.img')]:
  subprocess.run(['cp','--reflink=auto','--sparse=always',str(inp),str(r/'images'/name)],check=True)
  (r/'images'/name).chmod(0o400)
  if sha(r/'images'/name)!=(SYSTEM_SHA if name=='system.img' else VENDOR_SHA):raise SystemExit('Copied image verification failed')
 for rel in LIBS:
  out=r/'vendor-overlay/lib64'/rel;out.parent.mkdir(parents=True,exist_ok=True)
  shutil.copyfile(a.mesa_tree/rel,out);out.chmod(0o644)
 shutil.copyfile(manifest,r/'vendor-overlay/etc/vintf/manifest.xml')
 (r/'vendor-overlay/etc/vintf/manifest.xml').chmod(0o644)
 # Overlay directories become the visible /vendor modes inside Android.
 for directory in [r/'vendor-overlay',*(x for x in (r/'vendor-overlay').rglob('*') if x.is_dir())]:directory.chmod(0o755)
 shutil.copyfile(HERE/'baseline/waydroid.seccomp',r/'lxc'/r.name/'waydroid.seccomp')
 props=['sys.use_memfd=true','ro.adb.secure=1','ro.debuggable=0','debug.stagefright.ccodec=0','ro.opengles.version=196610','waydroid.updater.disabled=true','waydroid.tools_version=1.6.3','ro.vndk.lite=true','waydroid.stub_sensors_hal=1','waydroid.host.user='+host.pw_name,'waydroid.host.uid='+str(a.host_uid),'waydroid.host.gid='+str(host.pw_gid),'waydroid.host_data_path='+str(r/'data'),'waydroid.xdg_runtime_dir=/run/xdg','waydroid.pulse_runtime_path=/run/xdg/pulse','waydroid.wayland_display=wayland-0','waydroid.background_start=true']
 props+=['ro.hardware.egl=mesa','ro.hardware.gralloc=gbm','gralloc.gbm.device=/dev/dri/renderD128','ro.hardware.vulkan=freedreno'] if a.mode=='phone-gpu' else ['ro.hardware.egl=swiftshader','ro.hardware.gralloc=default']
 (r/'vendor-overlay/waydroid.prop').write_text('\n'.join(props)+'\n')
 (r/'vendor-overlay/waydroid.prop').chmod(0o644)
 lines=[f'lxc.rootfs.path = dir:{r}/rootfs','lxc.arch = aarch64','lxc.autodev = 0','lxc.mount.auto = cgroup:ro sys:ro proc','lxc.console.path = none',f'lxc.uts.name = {r.name}','lxc.apparmor.profile = unconfined',f'lxc.seccomp.profile = {r}/lxc/{r.name}/waydroid.seccomp','lxc.seccomp.allow_nesting = 1','lxc.no_new_privs = 1','lxc.init.cmd = /init','lxc.net.0.type = empty','lxc.pty.max = 10','lxc.cgroup2.memory.max = 2147483648','lxc.cgroup2.memory.swap.max = 536870912','lxc.cgroup2.cpu.max = 200000 100000','lxc.mount.entry = tmpfs dev tmpfs nosuid 0 0','lxc.mount.entry = none dev/pts devpts defaults,newinstance,mode=620,ptmxmode=666,create=dir 0 0']
 # FUSE is needed for Android's fresh emulated-storage setup; it is not GPU.
 # Android FirstStageMain itself creates random/urandom/kmsg; EEXIST is fatal.
 # Match Waydroid's own generated device list rather than precreating them.
 for n in ['zero','null','full','tty','fuse']:
  lines.append(f'lxc.mount.entry = /dev/{n} dev/{n} none bind,create=file 0 0')
 for n in ['binder','vndbinder','hwbinder']:
  lines.append(f'lxc.mount.entry = {r}/binderfs/{n} dev/{n} none bind,create=file 0 0')
 if a.mode=='phone-gpu':
  for n in ['dri/renderD128','dma_heap/system']:
   lines.append(f'lxc.mount.entry = /dev/{n} dev/{n} none bind,create=file 0 0')
 for n in ['tmp','var','run','mnt_extra']:
  lines.append(f'lxc.mount.entry = tmpfs {n} tmpfs nodev,create=dir 0 0')
 lines.append(f'lxc.mount.entry = {r}/data data none bind,create=dir 0 0')
 if sock:lines.append(f'lxc.mount.entry = {sock} run/xdg/wayland-0 none bind,create=file 0 0')
 (r/'lxc'/r.name/'config').write_text('\n'.join(lines)+'\n')
 (r/'trial.json').write_text(json.dumps(meta,indent=2)+'\n')
 shutil.copyfile(HERE/'run-trial.py',r/'run-trial.py')
 print('PREPARED ONLY. No mounts, binder ioctls, modules or containers started.')
 print('Review trial.json and lxc config before explicit run-trial.py --start-180s.')
if __name__=='__main__':main()
