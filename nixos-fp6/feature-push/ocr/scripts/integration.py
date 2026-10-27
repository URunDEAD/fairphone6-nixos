#!/usr/bin/env python3
"""Run under dbus-run-session; use only rendered synthetic fixtures."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import dbus

binary = os.environ.get("OCR_TEST_BINARY", str(Path("target/debug/fairphone-ocr").resolve()))
fixture = str(Path("evaluation/generated/romanian.png").resolve())
with tempfile.TemporaryDirectory(prefix="ocr-integration-") as tmp:
    isolated = {"XDG_CONFIG_HOME": tmp + "/config", "XDG_CACHE_HOME": tmp + "/cache", "XDG_DATA_HOME": tmp + "/data", "GSETTINGS_BACKEND": "memory"}
    for key, value in isolated.items():
        if key.startswith("XDG_"): Path(value).mkdir(mode=0o700)
    os.environ.update(isolated)
    bus = dbus.SessionBus()
    dbus.Interface(bus.get_object("org.freedesktop.DBus", "/org/freedesktop/DBus"), "org.freedesktop.DBus").UpdateActivationEnvironment(isolated)
    daemon = subprocess.Popen([binary, "serve", "--data-dir", tmp, "--screenshot-dir", str(Path(fixture).parent)])
    try:
        bus = dbus.SessionBus()
        for _ in range(100):
            if bus.name_has_owner("com.fairphone6.Ocr1"):
                break
            time.sleep(.1)
        proxy = dbus.Interface(bus.get_object("com.fairphone6.Ocr1", "/com/fairphone6/Ocr1"), "com.fairphone6.Ocr1")
        assert not json.loads(proxy.GetSettings())["index_enabled"]
        assert Path(str(proxy.LatestScreenshot())).exists()
        try:
            proxy.Recognize("https://invalid.example/never-fetched.png", "eng")
            raise AssertionError("Remote image accepted")
        except dbus.DBusException:
            pass
        try:
            proxy.Recognize(fixture, "invalid")
            raise AssertionError("Unsupported language accepted")
        except dbus.DBusException:
            pass
        def recognize():
            job_id = proxy.Recognize(fixture, "ron")
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                job = json.loads(proxy.GetJob(job_id))
                if job["status"] == "done": return job["result"]
                assert job["status"] != "failed", job
                time.sleep(.1)
            raise AssertionError("OCR job timed out")
        result = recognize()
        assert "România" in result["text"] and "învățăm" in result["text"], result
        assert not (Path(tmp) / "index.json").exists()
        proxy.SetIndexEnabled(True)
        recognize()
        assert len(json.loads(proxy.Search("ROMÂNIA"))) == 1
        proxy.SetIndexEnabled(False)
        assert json.loads(proxy.Search("România")) == []
        assert "România" not in (Path(tmp) / "index.json").read_text()
        canceled = proxy.Recognize(fixture, "ron")
        proxy.CancelJob(canceled)
        assert json.loads(proxy.GetJob(canceled))["status"] == "canceled"
        env = dict(os.environ, OCR_QMLSCENE=str(Path("scripts/ui_smoke.py").resolve()), OCR_QML_DIR=os.environ.get("OCR_TEST_QML_DIR", str(Path("qml").resolve())), XDG_CACHE_HOME=tmp, GSETTINGS_BACKEND="memory")
        subprocess.run([binary, "ui", fixture], env=env, check=True, timeout=120)
        print("D-Bus, OCR, indexing opt-in/purge, cancellation and UI integration: PASS")
    finally:
        daemon.terminate()
        daemon.wait(timeout=5)
