#!/usr/bin/env python3
"""Read-only capture of transient FP6 initrd ADB diagnostics."""
from pathlib import Path
import subprocess
import re
import sys
import time

adb = '/nix/store/p26kai4x3f28dfrhn300ww3bmga0hxcf-android-tools-35.0.2/bin/adb'
output = Path(sys.argv[1])
deadline = time.monotonic() + 150
last_size = 0
last_boot_id = None
while time.monotonic() < deadline:
    try:
        p = subprocess.run([adb, '-s', '0123456789', 'exec-out', 'sh', '-c',
                            "cat /proc/sys/kernel/random/boot_id; cat /proc/1/comm; cat /etc/os-release; cat /.error.json; mount; dmesg"],
                           capture_output=True, timeout=5)
        boot_id = p.stdout.split(b'\n', 1)[0]
        # ADB may briefly answer with an exec error during the root switch.
        # Preserve the last valid boot log instead of treating that as a new boot.
        if p.returncode != 0 or not re.fullmatch(
                rb'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', boot_id):
            time.sleep(0.5)
            continue
        if boot_id != last_boot_id:
            last_boot_id = boot_id
            last_size = 0
        if p.returncode == 0 and len(p.stdout) > last_size:
            output.write_bytes(p.stdout)
            last_size = len(p.stdout)
            print(f'Captured {last_size} bytes for boot {boot_id.decode(errors="replace")} to {output}', flush=True)
    except subprocess.TimeoutExpired:
        pass
    time.sleep(0.5)
