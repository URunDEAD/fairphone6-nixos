#!/usr/bin/env python3
"""Initialize a new NixOS Waydroid installation from verified public images.

No existing Android userdata is read or reused. Initialization never starts the
container, downloads an OTA image or changes the original postmarketOS files.
"""
import argparse
import configparser
import hashlib
import json
import os
import pwd
from pathlib import Path
import shutil
import subprocess
import time

IMAGES = {
    'system.img': '48af56f69d8f3f415e2278f39f6e4c3e26dde2d33902d0585f79a5872899b61d',
    'vendor.img': 'ad424225175307bbdd098b777962f05d208794bac67b9c1771ee1a2078fd4cc6',
}
LIBS = {'libgallium_dri.so', 'libgbm_mesa.so', 'dri_gbm.so',
        'egl/libEGL_mesa.so', 'egl/libGLESv1_CM_mesa.so',
        'egl/libGLESv2_mesa.so', 'hw/vulkan.freedreno.so'}
PROPERTIES = {'ro.hardware.egl': 'mesa', 'ro.hardware.gralloc': 'gbm',
              'gralloc.gbm.device': '/dev/dri/renderD128',
              'ro.hardware.vulkan': 'freedreno', 'sys.use_memfd': 'true',
              'debug.stagefright.ccodec': '0'}


def sha(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mesa', type=Path, required=True)
    parser.add_argument('--baseline-manifest', type=Path, required=True)
    parser.add_argument('--session-user', required=True, help='Phosh account whose new Android userdata will be used')
    parser.add_argument('--apply', action='store_true', help='Initialize; default only validates and prints the plan')
    args = parser.parse_args()
    assert os.geteuid() == 0, 'Run as root to inspect the protected public images'
    assert b'fairphone,fp6' in Path('/proc/device-tree/compatible').read_bytes().split(b'\0')
    account = pwd.getpwnam(args.session_user)
    assert account.pw_uid != 0, 'Android session must belong to the non-root Phosh account'
    user_state = Path(account.pw_dir) / '.local/share/waydroid'
    assert not user_state.exists() and not user_state.is_symlink(), 'Existing per-user Waydroid state: preserve it and review before changing it'
    source = Path('/run/nura/etc/waydroid-extra/images')
    images = Path('/var/lib/fp6-waydroid-images')
    work = Path('/var/lib/waydroid')
    assert work.parent.is_dir() and not work.parent.is_symlink()
    assert images.parent.is_dir() and not images.parent.is_symlink()
    assert not work.exists() and not work.is_symlink(), 'Existing Waydroid installation: preserve it and review before changing it'
    assert not images.exists() and not images.is_symlink(), 'Image staging already exists; retain it for review'
    assert Path('/etc/waydroid-extra/images').is_symlink()
    assert Path('/etc/waydroid-extra/images').resolve() == images
    for name, expected in IMAGES.items():
        path = source / name
        assert path.is_file() and not path.is_symlink() and sha(path) == expected, name
    manifest = json.loads((args.mesa / 'share/fp6-waydroid-a810/artifacts.json').read_text())
    assert set(manifest) == LIBS
    for name, entry in manifest.items():
        assert sha(args.mesa / 'lib64' / name) == entry['sha256'], name
    baseline = args.baseline_manifest
    assert sha(baseline) == '80d224effa0881b47d67a6cd2802945752214f21a42b2be961f9b05bbee682b9'
    text = baseline.read_text()
    assert 'android.hardware.media.omx' not in text
    report = {'new_work_directory': str(work), 'public_images': IMAGES,
              'mesa': str(args.mesa), 'libraries': sorted(LIBS),
              'properties': PROPERTIES, 'session_user': args.session_user,
              'new_user_state': str(user_state), 'existing_userdata_used': False,
              'container_started': False, 'automatic_download': False}
    print(json.dumps(report, indent=2), flush=True)
    if not args.apply:
        return
    assert os.uname().release == '7.2.0-fp6-nixos2', 'Use the separately tested GMU-timeout kernel first'
    assert Path('/sys/bus/platform/devices/3d00000.gpu/power/runtime_status').read_text().strip() in {'active', 'suspended'}
    os.umask(0o022)
    images.mkdir(mode=0o755)
    for name, expected in IMAGES.items():
        subprocess.run(['cp', '--reflink=auto', '--sparse=always', str(source/name), str(images/name)], check=True)
        (images/name).chmod(0o400)
        assert sha(images/name) == expected
    # The preinstalled-image path makes the pinned initializer avoid OTA fetches.
    subprocess.run(['waydroid', 'init'], check=True, timeout=120)
    assert work.is_dir()
    cfg_path = work / 'waydroid.cfg'
    cfg = configparser.ConfigParser(interpolation=None)
    cfg.read(cfg_path)
    assert cfg['waydroid']['images_path'] == '/etc/waydroid-extra/images'
    assert cfg['waydroid']['system_ota'] == cfg['waydroid']['vendor_ota'] == 'None'
    if not cfg.has_section('properties'):
        cfg.add_section('properties')
    cfg['properties'].update(PROPERTIES)
    with cfg_path.open('w') as stream:
        cfg.write(stream)
    overlay = work / 'overlay/vendor'
    for name in LIBS:
        destination = overlay / 'lib64' / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        assert not destination.exists() and not destination.is_symlink()
        shutil.copyfile(args.mesa / 'lib64' / name, destination)
        destination.chmod(0o644)
        assert sha(destination) == manifest[name]['sha256']
    destination = overlay / 'etc/vintf/manifest.xml'
    destination.parent.mkdir(parents=True, exist_ok=True)
    assert not destination.exists() and not destination.is_symlink()
    shutil.copyfile(baseline, destination)
    destination.chmod(0o644)
    # Regenerate base properties with the explicit GPU selectors; still stopped.
    subprocess.run(['waydroid', 'init'], check=True, timeout=120)
    cfg.read(cfg_path)
    assert cfg['waydroid']['images_path'] == '/etc/waydroid-extra/images'
    assert cfg['waydroid']['system_ota'] == cfg['waydroid']['vendor_ota'] == 'None'
    assert all(cfg['properties'][key] == value for key, value in PROPERTIES.items())
    base_props = dict(line.split('=', 1) for line in (work / 'waydroid_base.prop').read_text().splitlines() if '=' in line)
    assert all(base_props.get(key) == value for key, value in PROPERTIES.items())
    assert base_props.get('waydroid.updater.disabled') == 'true'
    assert sha(destination) == sha(baseline)
    assert not user_state.exists() and not user_state.is_symlink(), 'Initialization unexpectedly created user state'
    assert all(sha(overlay / 'lib64' / name) == manifest[name]['sha256'] for name in LIBS)
    report.update(initialized_at=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                  ready=True, baseline_manifest_sha256=sha(baseline))
    # Last step: permit the explicitly started, otherwise inactive service.
    marker = work / 'fp6-gpu-ready.json'
    pending_marker = work / '.fp6-gpu-ready.json.pending'
    with pending_marker.open('x') as stream:
        stream.write(json.dumps(report, indent=2) + '\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(pending_marker, marker)
    print('Initialized and verified. Android remains stopped.', flush=True)


if __name__ == '__main__':
    main()
