#!/usr/bin/env python3
"""Detached, root-only GPU Phosh -> GPU Lomiri TEST activation with Phosh rollback.

Adapted from switch-phone-phosh-renderer.py / switch-phone-phosh.py. Writing or
reviewing this helper does not authorize running it; only the parent agent (with
the user's approval) runs it on the phone. It never changes the saved system
profile, never selects/writes a boot image, slot or partition, and never starts
Waydroid. An independent systemd timer restores the old Phosh TEST root unless
the caller explicitly accepts after verifying the live Lomiri session.

  prepare:  switch-phone-lomiri-gpu.py /nix/store/...-nixos-system-fp6-lomiri-gpu-... \
                --hardware-probe-verified [--rollback-seconds 300]
  accept:   switch-phone-lomiri-gpu.py --accept STATE --session-verified
  rollback: switch-phone-lomiri-gpu.py --rollback STATE   (the timer does this itself)
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import sys
import time

BASE = Path('/var/lib/fp6-setup/lomiri-gpu-switches')
GPU_POWER = Path('/sys/bus/platform/devices/3d00000.gpu/power/runtime_status')
FORBIDDEN = {'LIBGL_ALWAYS_SOFTWARE', 'GBM_ALWAYS_SOFTWARE', 'GALLIUM_DRIVER',
             'MESA_LOADER_DRIVER_OVERRIDE', 'MESA_SHADER_CACHE_DISABLE',
             'LP_NUM_THREADS', '__EGL_VENDOR_LIBRARY_FILENAMES',
             'LIBGL_DRIVERS_PATH', 'WLR_RENDERER_ALLOW_SOFTWARE',
             'QT_QUICK_BACKEND', 'LD_LIBRARY_PATH', 'LD_PRELOAD'}
PREFIXES = ('QT_', 'QML', 'MIR_', 'MALIIT_', 'GRID_', 'DEVICEINFO_', 'GDK_',
            'GTK_', 'GIO_', 'GSK_', 'LD_', 'WLR_', 'MESA_', 'LIBGL_', 'GBM_')
EXPLICIT = FORBIDDEN | {'PATH', 'XDG_DATA_DIRS', 'XDG_CONFIG_DIRS',
            'XDG_CURRENT_DESKTOP', 'XDG_SESSION_DESKTOP', 'XDG_SESSION_TYPE',
            'XDG_SESSION_ID', 'XDG_SESSION_PATH', 'WAYLAND_DISPLAY', 'DISPLAY',
            'XMODIFIERS', 'GSETTINGS_SCHEMA_DIR', 'GTK_PATH', 'QT_IM_MODULE',
            'GTK_IM_MODULE', 'GIO_EXTRA_MODULES', 'NO_AT_BRIDGE', 'GTK_A11Y'}
# Old (Phosh) user jobs that must not survive into the Lomiri package set, and
# the reverse for rollback. LAL transient units may vanish while listed.
PHOSH_UNITS = r'(mobi\.phosh\.|sm\.puri\.|gnome-|org\.gnome\.|xdg-desktop-portal|feedbackd)'
LOMIRI_UNITS = r'(lomiri|ayatana-indicator-|dbus-.*com\.lomiri\.|maliit|mediascanner|' \
               r'lomiri-app-launch|application-|telephony|history-daemon|' \
               r'lomiri-url-dispatcher|lomiri-content-hub)'
USER = ['runuser', '-u', 'user', '--', 'env', 'XDG_RUNTIME_DIR=/run/user/10000',
        'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/10000/bus']
CTL = USER + ['systemctl', '--user']


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def run(command, check=True, timeout=45):
    return subprocess.run(command, check=check, timeout=timeout)


def output(command):
    return subprocess.check_output(command, text=True, timeout=30)


def protected(path, directory=False):
    info = path.lstat()
    require(not path.is_symlink() and info.st_uid == 0 and
            stat.S_IMODE(info.st_mode) & 0o077 == 0,
            'State must be root-owned, private, and not a symlink: ' + str(path))
    require(path.is_dir() if directory else path.is_file(), 'Invalid state object')


def state_path(value):
    path = Path(value)
    require(path.parent == BASE and re.fullmatch(r'[a-z0-9]{32}-[0-9]+', path.name),
            'Invalid state path')
    protected(BASE, True)
    protected(path, True)
    require(path.resolve() == path, 'State path traverses a symlink')
    protected(path / 'state.json')
    return path


def load(path):
    state = json.loads((path / 'state.json').read_text())
    protected(path / 'helper.py')
    require(hashlib.sha256((path / 'helper.py').read_bytes()).hexdigest() ==
            state['helper_sha256'], 'Private rollback helper changed')
    return state


def save(path, value):
    with (path / 'state.json').open('x') as stream:
        os.fchmod(stream.fileno(), 0o600)
        json.dump(value, stream, indent=2)
        stream.write('\n')


def boot_id():
    return Path('/proc/sys/kernel/random/boot_id').read_text().strip()


def current():
    return str(Path('/run/current-system').resolve())


def unchanged(state):
    require(boot_id() == state['boot_id'], 'Boot ID changed; refusing stale state')
    require(str(Path('/nix/var/nix/profiles/system').resolve()) == state['profile'],
            'Saved profile changed independently; refusing stale state')
    require(str(Path('/run/booted-system/kernel-modules').resolve()) == state['modules'],
            'Running booted module closure changed')


def root_path(value):
    path = Path(value)
    require(path.parent == Path('/nix/store') and
            re.fullmatch(r'[a-z0-9]{32}-nixos-system-fp6-.+', path.name),
            'Expected exact FP6 NixOS store root')
    require(path.resolve() == path and path.is_dir(), 'Root must exist without alias')
    return path


def same_kernel(root):
    require((root / 'kernel-modules').resolve() == Path('/run/booted-system/kernel-modules').resolve(),
            'Root does not use the exact running booted module closure')
    require((root / 'kernel').resolve() == Path('/run/booted-system/kernel').resolve(),
            'Root does not use the exact running booted kernel')
    require((root / 'kernel-modules/lib/modules' / os.uname().release).is_dir(),
            'Running kernel release missing from root modules')


def unit_environment(text):
    values = {}
    for line in text.splitlines():
        if line.startswith('Environment='):
            for item in shlex.split(line.partition('=')[2]):
                key, value = item.split('=', 1)
                values[key] = value
    return values


def forbidden(values):
    return FORBIDDEN.intersection(values) | {key for key in values if key.startswith('LD_')}


def validate_phosh(root):
    """The rollback target: the running hardware-rendered Phosh generation."""
    same_kernel(root)
    text = (root / 'etc/systemd/system/phosh.service').read_text()
    require('User=user' in text, 'Phosh service must run as user')
    values = unit_environment(text)
    require(values.get('WLR_RENDERER') == 'gles2', 'Rollback root is not GPU Phosh')
    require((root / 'etc/systemd/system/display-manager.service').resolve() == Path('/dev/null'),
            'Rollback root must mask the display manager')
    require((root / 'etc/systemd/user/mobi.phosh.OSK.service').is_file(), 'Missing Phosh OSK unit')
    return values


def validate_lomiri(root):
    same_kernel(root)
    phosh = root / 'etc/systemd/system/phosh.service'
    require(not phosh.exists() or phosh.resolve() == Path('/dev/null'),
            'Lomiri root must not ship an active Phosh unit')
    dm = root / 'etc/systemd/system/display-manager.service'
    require(dm.resolve() != Path('/dev/null'), 'Lomiri root masks the display manager')
    # NixOS starts LightDM through a unit-script wrapper; follow ExecStart.
    starts = re.findall(r'^ExecStart=(/nix/store/\S+)', dm.read_text(), re.M)
    require(starts and re.search(r'^exec /nix/store/\S+-lightdm-[^/]+/sbin/lightdm$',
                                 Path(starts[-1]).read_text(), re.M),
            'Lomiri root must run LightDM as display manager')
    conf = (root / 'etc/lightdm/lightdm.conf').read_text()
    wrappers = re.findall(r'^greeter-wrapper\s*=\s*(/nix/store/\S+)\s*$', conf, re.M)
    require(wrappers and Path(wrappers[-1]).is_file(), 'Missing FP6 Lomiri greeter wrapper')
    greeter = Path(wrappers[-1]).read_text()
    require(not forbidden(dict(re.findall(r'^export (\w+)=(.*)$', greeter, re.M))),
            'Greeter wrapper forces software rendering')
    require(re.search(r'^user-session\s*=\s*lomiri\s*$', conf, re.M), 'Default session is not lomiri')
    keyboard = (root / 'etc/systemd/user/lomiri-keyboard.service')
    require(keyboard.resolve() != Path('/dev/null'), 'Lomiri keyboard unit masked')
    require('QT_QUICK_BACKEND=software' not in keyboard.read_text(),
            'Keyboard unit still forces the software backend (fallback must be marker-only)')
    sessions = re.findall(r'^sessions-directory\s*=\s*(\S+)\s*$', conf, re.M)
    require(sessions and any((Path(d) / 'lomiri.desktop').is_file()
                             for d in sessions[-1].split(':')),
            'Lomiri session missing from LightDM sessions-directory')
    waydroid = root / 'etc/systemd/system/multi-user.target.wants/waydroid-container.service'
    require(not waydroid.exists(), 'Waydroid container must not start at boot')
    for app in ['lomiri', 'lomiri-weather-app', 'lomiri-dialer-app', 'lomiri-messaging-app',
                'lomiri-addressbook-app', 'morph-browser', 'lomiri-system-settings', 'fp6-waydroid-setup']:
        require((root / 'sw/bin' / app).exists(), 'Missing program ' + app)


def declarative_environment(root):
    # runuser supplies its normal user identity; no user login hooks are sourced.
    shell = '. ' + shlex.quote(str(root / 'etc/set-environment')) + '; env -0'
    raw = subprocess.check_output(['runuser', '-u', 'user', '--',
          str(root / 'sw/bin/bash'), '--noprofile', '--norc', '-c', shell],
          env={'PATH': '/run/current-system/sw/bin:/run/current-system/sw/sbin',
               'TERM': 'dumb'}, timeout=30)
    return dict(item.decode().split('=', 1) for item in raw.split(b'\0') if b'=' in item)


def publish(values, keys):
    clear = set(keys) - set(values)
    # D-Bus cannot unset: publish empty removed values there, then unset them in
    # the user manager. Done BEFORE test activation can start the new shell.
    run(USER + ['/run/current-system/sw/bin/dbus-update-activation-environment',
         '--systemd', *[key + '=' for key in sorted(clear)],
         *[key + '=' + value for key, value in sorted(values.items())]])
    if clear:
        run(CTL + ['unset-environment', *sorted(clear)])


def stop_user_units(pattern):
    units = output(CTL + ['list-units', '--state=active', '--plain', '--no-legend', '--no-pager'])
    stale = [line.split()[0] for line in units.splitlines() if line and re.match(pattern, line)]
    if stale:
        run(CTL + ['stop', *stale], check=False)


def stop_phosh():
    run(['systemctl', 'stop', 'phosh.service'])
    run(CTL + ['stop', 'graphical-session.target'], check=False)
    stop_user_units(PHOSH_UNITS)


def stop_lomiri():
    run(['systemctl', 'stop', 'display-manager.service'], check=False)
    run(CTL + ['stop', 'lomiri.service', 'graphical-session.target'], check=False)
    stop_user_units(LOMIRI_UNITS)


def rollback(path, state):
    unchanged(state)
    if (path / 'accepted-test').exists():
        return
    if (path / 'rolled-back').exists():
        require(current() == state['old_system'], 'State says rolled back but current root differs')
        return
    require(current() in {state['old_system'], state['new_system']},
            'Another system was activated; refusing unrelated rollback')
    errors = []
    try:
        stop_lomiri()
    except BaseException as error:
        errors.append(('Lomiri stop', error))
    try:
        publish(state['old_environment'], state['changed_keys'])
    except BaseException as error:
        errors.append(('old environment publish', error))
    try:
        run([state['old_system'] + '/bin/switch-to-configuration', 'test'], timeout=120)
    except BaseException as error:
        errors.append(('Phosh TEST activation', error))
    finally:
        try:
            unchanged(state)
            require(current() == state['old_system'], 'Phosh rollback root mismatch; refusing start')
            validate_phosh(root_path(state['old_system']))
        except BaseException as error:
            errors.append(('Phosh fallback safety checks', error))
        else:
            for command, phase in [(['systemctl', 'daemon-reload'], 'system daemon reload'),
                                   (CTL + ['daemon-reload'], 'user daemon reload'),
                                   (['systemctl', 'stop', 'display-manager.service'], 'LightDM stop'),
                                   (['systemctl', 'start', 'phosh.service'], 'Phosh start'),
                                   (['systemctl', 'is-active', '--quiet', 'phosh.service'],
                                    'Phosh active check')]:
                try:
                    run(command, check=phase != 'LightDM stop')
                except BaseException as error:
                    errors.append((phase, error))
    if errors:
        # Keep the independent timer armed and preserve the original error first.
        details = [{'phase': phase, 'error': repr(error)} for phase, error in errors]
        try:
            (path / ('rollback-failed-' + str(time.monotonic_ns()) + '.json')).write_text(
                json.dumps(details, indent=2) + '\n')
        except OSError as error:
            errors[0][1].add_note('Could not save rollback error report: ' + repr(error))
        for phase, error in errors[1:]:
            errors[0][1].add_note(phase + ': ' + repr(error))
        raise errors[0][1]
    (path / 'rolled-back').write_text('GPU Phosh TEST root restored; saved profile unchanged.\n')
    run(['systemctl', 'stop', state['rollback_unit'] + '.timer'], check=False)


def activate(path, state):
    unchanged(state)
    require(state['hardware_probe_verified_by_caller'], 'Missing caller hardware proof')
    require(not any((path / name).exists() for name in
            ['activated-test', 'accepted-test', 'rolled-back']),
            'Do not reuse completed state; prepare a unique new trial')
    run(['systemctl', 'is-active', '--quiet', state['rollback_unit'] + '.timer'])
    require(current() == state['old_system'], 'Phosh root changed before activation')
    require(GPU_POWER.read_text().strip() in {'active', 'suspended'}, 'GPU runtime is not healthy')
    try:
        stop_phosh()
        publish(state['new_environment'], state['changed_keys'])
        run([state['new_system'] + '/bin/switch-to-configuration', 'test'], timeout=120)
        run(['systemctl', 'daemon-reload'])
        run(CTL + ['daemon-reload'])
        run(['systemctl', 'start', 'display-manager.service'])
        run(['systemctl', 'is-active', '--quiet', 'display-manager.service'])
        unchanged(state)
        require(current() == state['new_system'], 'Lomiri TEST root mismatch')
        (path / 'activated-test').write_text(
            'Lomiri TEST activation complete; greeter login and GPU acceptance still pending.\n')
    except BaseException as activation_error:
        try:
            rollback(path, state)
        except BaseException as rollback_error:
            activation_error.add_note('Immediate Phosh rollback also failed: ' + repr(rollback_error))
            raise activation_error from rollback_error
        raise


def process_env(pid):
    return dict(item.decode(errors='replace').split('=', 1) for item in
                Path(f'/proc/{pid}/environ').read_bytes().split(b'\0') if b'=' in item)


def accept(path, state, verified):
    require(verified, '--session-verified requires the caller to check the live GPU session first')
    unchanged(state)
    require((path / 'activated-test').exists() and not (path / 'rolled-back').exists(),
            'No pending successful Lomiri TEST activation')
    require(current() == state['new_system'], 'Lomiri TEST root not current')
    require(GPU_POWER.read_text().strip() in {'active', 'suspended'}, 'GPU runtime is not healthy')
    run(['systemctl', 'is-active', '--quiet', 'display-manager.service'])
    run(CTL + ['is-active', '--quiet', 'lomiri.service'])
    manager = dict(line.split('=', 1) for line in output(CTL + ['show-environment']).splitlines() if '=' in line)
    require(not forbidden(manager), 'User manager retains software-rendering keys')
    pid = int(output(CTL + ['show', 'lomiri.service', '-p', 'MainPID', '--value']))
    require(pid > 0, 'Missing live Lomiri process')
    # lomiri-session is a wrapper script; check every process of the unit.
    group = output(CTL + ['show', 'lomiri.service', '-p', 'ControlGroup', '--value']).strip()
    pids = [int(p) for p in Path('/sys/fs/cgroup' + group + '/cgroup.procs').read_text().split()]
    shell = None
    for each in pids:
        require(not forbidden(process_env(each)), f'Live Lomiri process {each} has software-rendering keys')
        # Nix wrapProgram names the real binary .lomiri-wrapped.
        if Path(f'/proc/{each}/comm').read_text().strip() in {'lomiri', '.lomiri-wrapped'}:
            shell = each
    require(shell, 'No lomiri shell process in lomiri.service')
    maps = Path(f'/proc/{shell}/maps').read_text()
    require('libgallium' in maps or '_dri.so' in maps, 'Lomiri did not load a Mesa driver')
    fds = {os.readlink(f'/proc/{shell}/fd/{fd}') for fd in os.listdir(f'/proc/{shell}/fd')
           if os.path.islink(f'/proc/{shell}/fd/{fd}')}
    require(fds & {'/dev/dri/card0', '/dev/dri/renderD128'}, 'Lomiri holds no DRM device')
    run(['systemctl', 'stop', state['rollback_unit'] + '.timer'])
    (path / 'accepted-test').write_text(
        'Caller verified live GPU Lomiri session; TEST accepted, profile unchanged.\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('system', nargs='?')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--activate')
    modes.add_argument('--rollback')
    modes.add_argument('--accept')
    parser.add_argument('--hardware-probe-verified', action='store_true')
    parser.add_argument('--session-verified', action='store_true')
    # Greeter PIN entry happens on the physical phone, so allow more than Phosh's 180s.
    parser.add_argument('--rollback-seconds', type=int, choices=[180, 300, 600], default=300)
    args = parser.parse_args()
    require(os.geteuid() == 0, 'Run as root')
    os.umask(0o077)
    os.environ['PATH'] = '/run/current-system/sw/bin:/run/current-system/sw/sbin:' + os.environ.get('PATH', '')
    require(b'fairphone,fp6' in Path('/proc/device-tree/compatible').read_bytes().split(b'\0'), 'Not an FP6')
    value = args.activate or args.rollback or args.accept
    if value:
        require(not args.system, 'State operation cannot also select a root')
        path = state_path(value)
        with (path / 'operation.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            state = load(path)
            if args.activate:
                activate(path, state)
            elif args.rollback:
                rollback(path, state)
            else:
                accept(path, state, args.session_verified)
        print(json.dumps({'state': str(path), 'current_system': current(),
                          'profile_unchanged': state['profile']}), flush=True)
        return
    require(args.system and args.hardware_probe_verified,
            'Select the Lomiri root and pass --hardware-probe-verified only after a healthy GPU probe')
    old = root_path(current())
    new = root_path(args.system)
    require(old != new, 'Lomiri target is already current')
    old_unit = validate_phosh(old)
    validate_lomiri(new)
    require((old / 'initrd').resolve() == (new / 'initrd').resolve(), 'Phosh/Lomiri initrd differs')
    require(not list(Path('/run/systemd/system/display-manager.service.d').glob('*.conf')),
            'Unexpected runtime display-manager overrides')
    require(not list(Path('/run/user/10000/systemd/user/lomiri.service.d').glob('*.conf')),
            'Stale runtime lomiri.service drop-ins (old CPU trials) must be reviewed first')
    require(GPU_POWER.read_text().strip() in {'active', 'suspended'}, 'GPU runtime is not healthy')
    require(subprocess.run(['systemctl', 'is-active', '--quiet', 'waydroid-container.service']).returncode != 0,
            'Waydroid is running; stop it explicitly before changing the shell')
    manager = dict(line.split('=', 1) for line in output(CTL + ['show-environment']).splitlines() if '=' in line)
    fresh = declarative_environment(new)
    require(not forbidden(fresh), 'Lomiri session environment forces software rendering')
    keys = {key for key in set(manager) | set(fresh) | set(old_unit)
            if key in EXPLICIT or key.startswith(PREFIXES)} | EXPLICIT
    values = {key: value for key, value in fresh.items() if key in keys}
    BASE.mkdir(mode=0o700, parents=True, exist_ok=True)
    protected(BASE, True)
    require(BASE.resolve() == BASE, 'State base traverses a symlink')
    path = BASE / (new.name[:32] + '-' + str(time.monotonic_ns()))
    path.mkdir(mode=0o700)
    helper = path / 'helper.py'
    helper.write_bytes(Path(__file__).read_bytes())
    helper.chmod(0o700)
    timer = 'fp6-lomiri-gpu-rollback-' + path.name
    state = {'old_system': str(old), 'new_system': str(new),
             'profile': str(Path('/nix/var/nix/profiles/system').resolve()),
             'boot_id': boot_id(), 'modules': str((old / 'kernel-modules').resolve()),
             'old_environment': {key: value for key, value in manager.items() if key in keys},
             'new_environment': values, 'changed_keys': sorted(keys), 'rollback_unit': timer,
             'hardware_probe_verified_by_caller': True,
             'helper_sha256': hashlib.sha256(helper.read_bytes()).hexdigest()}
    save(path, state)
    for root in [old, new]:
        link = Path('/nix/var/nix/gcroots') / ('fp6-lomiri-gpu-' + root.name[:32])
        if link.is_symlink():
            require(link.resolve() == root, 'Existing GC root points elsewhere')
        else:
            require(not link.exists(), 'GC root name occupied')
            link.symlink_to(root)
    # Timer is armed before the detached worker stops Phosh. Both execute a
    # private root-owned copy, surviving SSH/USB-network restarts.
    run(['systemd-run', '--unit=' + timer, '--on-active=' + str(args.rollback_seconds) + 's',
         '--timer-property=AccuracySec=1s', '--service-type=oneshot',
         '--property=TimeoutStartSec=180s', sys.executable, str(helper), '--rollback', str(path)])
    try:
        run(['systemd-run', '--no-block', '--unit=fp6-lomiri-gpu-activate-' + path.name,
             '--service-type=oneshot', '--property=TimeoutStartSec=180s',
             '--property=KillMode=control-group', sys.executable, str(helper), '--activate', str(path)])
    except BaseException:
        (path / 'launch-failed').write_text('Activation launcher failed; rollback timer retained.\n')
        raise
    print(json.dumps({'state': str(path), 'old_phosh_system': str(old), 'new_lomiri_system': str(new),
                      'rollback_timer': timer + '.timer', 'rollback_seconds': args.rollback_seconds,
                      'detached': True, 'profile_unchanged': state['profile'], 'boot_writes': False}), flush=True)


if __name__ == '__main__':
    main()
