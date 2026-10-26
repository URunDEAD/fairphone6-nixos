#!/usr/bin/env python3
"""Explicit, bounded isolated trial. Never loads modules or starts a host service."""
import argparse,fcntl,json,os,re,signal,subprocess,sys,time
from pathlib import Path
R=Path(__file__).resolve().parent

def run(args,check=True,**kw):
 print('+',' '.join(map(str,args)),flush=True)
 return subprocess.run(list(map(str,args)),check=check,**kw)

def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--start-180s',action='store_true',required=True)
 p.add_argument('--ui-probe-settings',action='store_true')
 p.add_argument('--inside-mount-namespace',action='store_true',help=argparse.SUPPRESS)
 a=p.parse_args()
 if R.parent!=Path('/var/lib/fp6-waydroid-trials') or not R.name.startswith('fp6-'):raise SystemExit('Run only the prepared trial-local script')
 if os.geteuid()!=0:raise SystemExit('Root required')
 meta=json.loads((R/'trial.json').read_text())
 if meta['root']!=str(R) or meta['name']!=R.name:raise SystemExit('Trial path/manifest mismatch')
 if a.ui_probe_settings and meta['mode']!='vm-software':raise SystemExit('UI probe is isolated VM only')
 if not a.inside_mount_namespace:
  command=['unshare','--mount','--propagation','private',sys.executable,__file__,'--start-180s','--inside-mount-namespace']
  if a.ui_probe_settings:command.append('--ui-probe-settings')
  raise SystemExit(subprocess.call(command))
 if 'binder' not in Path('/proc/filesystems').read_text():raise SystemExit('binderfs unavailable: provision the reviewed kernel/module first; this script never modprobes')
 if meta['mode']=='vm-software':
  available=int(next(line.split()[1] for line in Path('/proc/meminfo').read_text().splitlines() if line.startswith('MemAvailable:')))
  if available<5*1024*1024:raise SystemExit('VM trial needs at least5GiB MemAvailable')
 if meta['mode']=='phone-gpu':
  status=Path('/sys/bus/platform/devices/3d00000.gpu/power/runtime_status')
  if not status.exists() or status.read_text().strip() not in ('active','suspended'):raise SystemExit('Refusing trial with unhealthy/unknown physical GPU state')
  for n in ['/dev/dri/renderD128','/dev/dma_heap/system']:
   if not Path(n).exists():raise SystemExit('Missing required new-kernel device '+n)
 # Refuse reuse after a prior test: test-specific data must begin empty.
 if any((R/'data').iterdir()):raise SystemExit('Trial data is nonempty; prepare a NEW trial instead')
 lxc=['-P',str(R/'lxc'),'-n',R.name]
 old=subprocess.run(['lxc-info',*lxc,'-sH'],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 if old.returncode==0 and old.stdout.strip()!='STOPPED':raise SystemExit('Trial already running')
 lock=(R/'run.lock').open('a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
 # Also prevents a second run with empty data after an early failed start.
 if (R/'started-once').exists():raise SystemExit('Prepare a new trial for each run')
 (R/'started-once').write_text(time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime())+'\n')
 proc=None;mounts=[];payload_cgroups=set();known_pids={};cleanup_errors=[];boot_complete=False;observations={};ui_probe={}
 def remember_pid(pid):
  try:
   known_pids[pid]=Path(f'/proc/{pid}/stat').read_text().split(') ')[1].split()[19]
  except (FileNotFoundError,IndexError):return
 def remember_payload():
  result=subprocess.run(['lxc-info',*lxc,'-pH'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,timeout=3)
  if result.returncode or not result.stdout.strip().isdigit():return
  pid=int(result.stdout.strip());remember_pid(pid)
  try:
   entry=Path(f'/proc/{pid}/cgroup').read_text().splitlines()
   for line in entry:
    if not line.startswith('0::'):continue
    cg=Path('/sys/fs/cgroup')/line[3:].lstrip('/')
    # Never kill an ancestor/shared cgroup; accept only the exact trial name.
    if not any(component in (R.name,'lxc.payload.'+R.name) for component in cg.parts):
     raise RuntimeError('Unrecognized payload cgroup: '+str(cg))
    payload_cgroups.add(cg)
    limits={n:(cg/n).read_text().strip() for n in ['memory.max','memory.swap.max','cpu.max']}
    if limits!={'memory.max':'2147483648','memory.swap.max':'536870912','cpu.max':'200000 100000'}:
     raise RuntimeError('Trial resource limits not applied: '+str(limits))
    for node in [cg,*cg.rglob('*')]:
     if node.is_dir() and (node/'cgroup.procs').exists():
      for item in (node/'cgroup.procs').read_text().split():remember_pid(int(item))
   (R/'logs/resources.json').write_text(json.dumps({'foreground_pid':proc.pid,'known_pids':known_pids,'payload_cgroups':list(map(str,payload_cgroups))},indent=2)+'\n')
  except FileNotFoundError:pass
 def mount(args,target):
  run(['mount',*args,target]);mounts.append(target)
 def attach(label,command):
  with (R/'logs'/label).open('w') as f:
   return run(['timeout','--kill-after=2','8','lxc-attach',*lxc,'--',*command],check=False,stdout=f,stderr=subprocess.STDOUT).returncode
 def interrupted(signum,frame):raise KeyboardInterrupt
 signal.signal(signal.SIGTERM,interrupted)
 try:
  mount(['-o','loop,ro,noload,nodev',R/'images/system.img'],R/'lower-system')
  mount(['-o','loop,ro,noload,nodev',R/'images/vendor.img'],R/'lower-vendor')
  mount(['-t','overlay','overlay','-o',f'lowerdir={R}/lower-system,upperdir={R}/upper-system,workdir={R}/work-system'],R/'rootfs')
  mount(['-t','overlay','overlay','-o',f'lowerdir={R}/vendor-overlay:{R}/lower-vendor,upperdir={R}/upper-vendor,workdir={R}/work-vendor'],R/'rootfs/vendor')
  mount(['-t','binder','binder'],R/'binderfs')
  # _IOWR('b',1,struct binderfs_device{char name[256];u32 major,minor;})
  # Dedicated binder contexts; never use the live Waydroid binder devices.
  with (R/'binderfs/binder-control').open('rb+',buffering=0) as ctl:
   for n in ['binder','vndbinder','hwbinder']:
    buf=bytearray(264);buf[:len(n)]=n.encode();fcntl.ioctl(ctl.fileno(),0xc1086201,buf,True)
    os.chmod(R/'binderfs'/n,0o666)
  with (R/'logs/lxc-console.txt').open('w') as log:
   proc=subprocess.Popen(['lxc-start',*lxc,'-F','-l','INFO','-o',str(R/'logs/lxc-debug.txt')],stdout=log,stderr=subprocess.STDOUT)
   remember_pid(proc.pid)
   deadline=time.monotonic()+180;boot_complete=False
   while time.monotonic()<deadline and proc.poll() is None:
    time.sleep(3)
    remember_payload()
    x=subprocess.run(['timeout','--kill-after=1','3','lxc-attach',*lxc,'--','/system/bin/getprop','sys.boot_completed'],stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
    if x.returncode==0 and x.stdout.strip()=='1':boot_complete=True;break
   if a.ui_probe_settings and boot_complete and deadline-time.monotonic()>60:
    # Same fullscreen selection as Waydroid's showFullUI, only in fresh data.
    ui_probe['fullscreen_selection']=attach('ui-fullscreen-selection.txt',['/system/bin/setprop','waydroid.active_apps','Waydroid'])
    ui_probe['launch_settings']=attach('ui-settings-launch.txt',['/system/bin/am','start','-W','-a','android.settings.SETTINGS'])
    time.sleep(3)
    ui_probe['activity_dump']=attach('ui-activity.txt',['/system/bin/dumpsys','activity','activities'])
    ui_probe['window_dump']=attach('ui-window.txt',['/system/bin/dumpsys','window','windows'])
    ui_probe['hierarchy_dump']=attach('ui-hierarchy-command.txt',['/system/bin/uiautomator','dump','/data/local/tmp/fp6-settings-ui.xml'])
    ui_probe['hierarchy_read']=attach('ui-hierarchy.xml',['/system/bin/cat','/data/local/tmp/fp6-settings-ui.xml'])
    with (R/'logs/android-settings.png').open('wb') as png,(R/'logs/screencap-stderr.txt').open('w') as errors:
     ui_probe['screencap']=run(['timeout','--kill-after=2','8','lxc-attach',*lxc,'--','/system/bin/screencap','-p'],check=False,stdout=png,stderr=errors).returncode
    (R/'logs/ui-capture-ready').write_text('Settings launched and Android PNG captured; private host capture window active.\n')
    # Permit cached Weston capture/debug clients to observe the same app surface.
    time.sleep(min(35,max(0,deadline-time.monotonic()-10)))
   for label,cmd in [
     ('getprop.txt',['/system/bin/getprop']),
     ('services.txt',['/system/bin/service','list']),
     ('surfaceflinger.txt',['/system/bin/dumpsys','SurfaceFlinger']),
     ('packages.txt',['/system/bin/cmd','package','list','packages']),
     ('codec-registry.txt',['/system/bin/dumpsys','media.codec']),
     ('logcat.txt',['/system/bin/logcat','-d','-t','400'])]:observations[label]=attach(label,cmd)
   (R/'logs/result.json').write_text(json.dumps({'boot_completed':boot_complete,'observations_exit_status':observations,'ui_probe_settings':ui_probe,'mode':meta['mode'],'timeout_seconds':180,'userdata':str(R/'data'),'gpu_acceptance':'Requires Adreno810 renderer, actual frames and clean kernel logs; boot property alone is insufficient','codec_acceptance':'Registry only; no hardware decode claim'},indent=2)+'\n')
 finally:
  if proc is not None:
   # Init can die before the first polling interval. Its exact PID/cgroup
   # are still recoverable from this trial's own LXC log/configuration.
   debug=R/'logs/lxc-debug.txt'
   log_init_pids=[int(x) for x in re.findall(r'Started "/init" with pid "(\d+)"',debug.read_text() if debug.exists() else '')]
   for pid in log_init_pids:remember_pid(pid)
   for cg in Path('/sys/fs/cgroup').rglob('lxc.payload.'+R.name):
    if cg.is_dir():payload_cgroups.add(cg)
   run(['timeout','--kill-after=2','10','lxc-stop',*lxc,'-k'],check=False)
   for cg in payload_cgroups:
    try:
     if 'populated 1' in (cg/'cgroup.events').read_text():
      (cg/'cgroup.kill').write_text('1\n')
    except FileNotFoundError:pass
    except OSError as error:cleanup_errors.append('payload kill: '+str(error))
   try:proc.wait(timeout=5)
   except subprocess.TimeoutExpired:
    proc.terminate()
    try:proc.wait(timeout=3)
    except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
   state=subprocess.run(['timeout','--kill-after=1','5','lxc-info',*lxc,'-sH'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
   if state.returncode or state.stdout.strip()!='STOPPED':cleanup_errors.append('LXC state not proven STOPPED: '+state.stdout+state.stderr)
   for cg in payload_cgroups:
    try:
     if 'populated 1' in (cg/'cgroup.events').read_text():cleanup_errors.append('Cgroup still populated: '+str(cg))
     for node in [cg,*cg.rglob('*')]:
      if node.is_dir() and (node/'cgroup.procs').exists() and (node/'cgroup.procs').read_text().strip():cleanup_errors.append('Live payload PIDs: '+str(node))
    except FileNotFoundError:pass
   for pid,identity in known_pids.items():
    try:
     if Path(f'/proc/{pid}/stat').read_text().split(') ')[1].split()[19]==identity:cleanup_errors.append('Observed trial PID still alive: '+str(pid))
    except (FileNotFoundError,IndexError):pass
   for pid in log_init_pids:
    if Path(f'/proc/{pid}').exists():cleanup_errors.append('Logged init PID still present: '+str(pid))
  for target in reversed(mounts):
   if run(['timeout','--kill-after=2','10','umount',target],check=False).returncode:cleanup_errors.append('Unmount failed: '+str(target))
  remaining=[line for line in Path('/proc/self/mountinfo').read_text().splitlines() if line.split()[4].startswith(str(R)+'/')]
  if remaining:cleanup_errors.append('Remaining trial mounts: '+repr(remaining))
  report={'status':'FAIL' if cleanup_errors else 'PASS','errors':cleanup_errors,'payload_cgroups':list(map(str,payload_cgroups)),'known_pids':known_pids,'logged_init_pids':log_init_pids if proc is not None else [],'mounts_left':remaining}
  (R/'logs/cleanup.json').write_text(json.dumps(report,indent=2)+'\n')
  if cleanup_errors:raise RuntimeError('Cleanup FAILED; retain resource IDs in logs/cleanup.json: '+repr(cleanup_errors))
 print('Cleanup PASS: exact container STOPPED, observed child PIDs gone, no populated payload cgroups or private mounts. Fresh data/logs retained.')
 if not boot_complete or any(observations.get(k)!=0 for k in ['getprop.txt','services.txt','packages.txt']):
  raise SystemExit('Android boot gate FAIL; inspect private logs/result.json and logcat.txt')
 if a.ui_probe_settings and (not ui_probe or any(ui_probe.get(k)!=0 for k in ['fullscreen_selection','launch_settings','activity_dump','window_dump','screencap'])):
  raise SystemExit('Settings UI probe incomplete; inspect fresh private logs')
 print('Android basic boot gate PASS; rendering and hardware remain separate checks.')
if __name__=='__main__':main()
