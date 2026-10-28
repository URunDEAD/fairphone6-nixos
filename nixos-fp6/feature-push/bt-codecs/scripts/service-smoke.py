#!/usr/bin/env python3
"""Exercise the installed Rust daemon with a private bus and no audio socket."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

with tempfile.TemporaryDirectory(prefix="fp6-bt-service-test-") as tmp:
    config = Path(tmp) / "bus.conf"
    config.write_text('''<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen>
    <auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/>
    <allow receive_sender="*"/><allow own="*"/></policy></busconfig>''')
    bus = subprocess.Popen(["dbus-daemon", "--nofork", "--print-address=1", "--config-file=" + str(config)], stdout=subprocess.PIPE, text=True)
    daemon = None
    try:
        address = bus.stdout.readline().strip()
        env = dict(os.environ, DBUS_SESSION_BUS_ADDRESS=address, XDG_RUNTIME_DIR=tmp)
        daemon = subprocess.Popen([sys.argv[1]], env=env)
        base = ["dbus-send", "--session", "--print-reply", "--reply-timeout=2000", "--dest=com.fairphone6.BluetoothAudio1", "/com/fairphone6/BluetoothAudio1"]
        for attempt in range(100):
            result = subprocess.run(base + ["org.freedesktop.DBus.Introspectable.Introspect"], env=env, capture_output=True, text=True)
            if result.returncode == 0:
                break
            time.sleep(.02)
        assert result.returncode == 0, result.stderr
        assert all(name in result.stdout for name in ["ListDevices", "SetProfile", "SetPreference"])
        for _ in range(3):
            result = subprocess.run(base + ["com.fairphone6.BluetoothAudio1.ListDevices"], env=env, capture_output=True, text=True)
            assert result.returncode != 0 and "PipeWire Pulse server unavailable" in result.stderr, result.stderr
        assert not (Path(tmp) / "pulse").exists(), "Status read created an audio socket"
        assert daemon.poll() is None
        print("Installed service: introspection and 3 bounded unavailable-server reads passed; no audio autospawn")
    finally:
        if daemon is not None:
            daemon.terminate()
            daemon.wait(timeout=5)
        bus.terminate()
        bus.wait(timeout=5)
