import json, os, signal, subprocess, sys, tempfile, time
from pathlib import Path
import dbus
prefix = Path(sys.argv[1]).resolve()
app = str(prefix / 'bin/fairphone-ocr')
fixture = str(Path('evaluation/generated/romanian.png').resolve())
assert 'X-Lomiri-Touch=true' in (prefix / 'share/applications/fairphone-ocr.desktop').read_text()
assert '"pictures"' in (prefix / 'share/lomiri-content-hub/peers/fairphone-ocr').read_text()
activation = (prefix / 'share/dbus-1/services/com.fairphone6.Ocr1.service').read_text()
assert app + ' serve' in activation and '@out@' not in activation
result = json.loads(subprocess.check_output([app, 'recognize', fixture, '--lang', 'ron', '--json'], text=True))
assert 'În România' in result['text'] and result['words']
with tempfile.TemporaryDirectory(prefix='ocr-package-') as tmp:
    env = dict(os.environ, TMPDIR=tmp, XDG_CONFIG_HOME=tmp+'/config', XDG_DATA_HOME=tmp+'/data', XDG_CACHE_HOME=tmp+'/cache', XDG_DATA_DIRS=str(prefix / 'share')+':'+os.environ.get('XDG_DATA_DIRS',''), GSETTINGS_BACKEND='memory')
    for name in ['config', 'data', 'cache']: Path(tmp, name).mkdir(mode=0o700)
    peers = Path(tmp, 'data/lomiri-content-hub')
    peers.mkdir(mode=0o700)
    (peers / 'fairphone-ocr').write_bytes((prefix / 'share/lomiri-content-hub/peers/fairphone-ocr').read_bytes())
    bus = dbus.SessionBus()
    changes = {key:value for key,value in env.items() if key.startswith('XDG_') or key=='GSETTINGS_BACKEND'}
    dbus.Interface(bus.get_object('org.freedesktop.DBus', '/org/freedesktop/DBus'), 'org.freedesktop.DBus').UpdateActivationEnvironment(changes)
    service = subprocess.Popen([app, 'serve', '--data-dir', tmp+'/index'], env=env)
    try:
        for _ in range(100):
            if bus.name_has_owner('com.fairphone6.Ocr1'): break
            time.sleep(.1)
        assert bus.name_has_owner('com.fairphone6.Ocr1')
        with tempfile.TemporaryFile(mode='w+') as log:
            gui = subprocess.Popen([app, 'ui', fixture], env=env, stdout=log, stderr=log, start_new_session=True)
            try:
                time.sleep(12)
                assert gui.poll() is None, 'Packaged GUI exited unexpectedly'
                hub = dbus.Interface(bus.get_object('com.lomiri.content.dbus.Service', '/'), 'com.lomiri.content.dbus.Service')
                assert any('fairphone-ocr' in str(peer) for peer in hub.KnownSharesForType('pictures')), 'Image share target was not registered'
                assert any('fairphone-ocr' in str(peer) for peer in hub.KnownDestinationsForType('pictures')), 'Image destination was not registered'
            finally:
                os.killpg(gui.pid, signal.SIGTERM)
                gui.wait(timeout=10)
            log.seek(0)
            output = log.read()
            for failure in ['TypeError:', 'ReferenceError:', 'Cannot assign', 'is not installed', 'Type App.Main unavailable', 'No peer setting directories exist.']:
                assert failure not in output, output
    finally:
        service.terminate()
        service.wait(timeout=10)
print('Packaged CLI, desktop/Content Hub/D-Bus metadata, service and actual qmlscene launcher: PASS')
