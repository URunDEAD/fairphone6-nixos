#!/usr/bin/env python3
"""Real worker + synthetic PCM, fake pw-record, private D-Bus. Needs a preinstalled model."""
import json, os, pathlib, signal, subprocess, sys, tempfile, time
if os.environ.get('CAPTIONS_SMOKE_PRIVATE') != '1':
    env = dict(os.environ, CAPTIONS_SMOKE_PRIVATE='1', DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent/captions-test-system')
    sys.exit(subprocess.call(['dbus-run-session', '--config-file', str(pathlib.Path(__file__).resolve().parent.parent / 'tests/bus.conf'), '--', sys.executable, __file__], env=env))
binary = os.environ.get('CAPTIONS_BIN', '/tmp/fp6-captions-target/debug/fp6-captions')
unwrapped = pathlib.Path(binary).with_name('.fp6-captions-wrapped')
if unwrapped.is_file():
    binary = str(unwrapped)  # Preserve the synthetic recorder PATH instead of the Nix wrapper's PATH.
def wait_until(check, timeout=70):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError('Timed out waiting for synthetic session; status=' + str({k:v for k,v in (status() or {}).items() if k != 'text'}))
with tempfile.TemporaryDirectory(prefix='fp6-captions-session-') as tmp:
    work = pathlib.Path(tmp)
    subprocess.run(['espeak-ng', '-v', 'en', '-s', '145', '-w', str(work/'raw.wav'), 'The train arrives at the station in ten minutes. I will meet you there.'], check=True)
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', str(work/'raw.wav'), '-ar', '16000', '-ac', '1', '-f', 's16le', str(work/'speech.pcm')], check=True)
    fake = work/'pw-record'
    fake.write_text('#!' + sys.executable + '\n' + '''import os, pathlib, sys, time, json
p=pathlib.Path(os.environ['CAPTIONS_FAKE_DIR'])
(p/'capture.json').write_text(json.dumps({'args':sys.argv[1:],'pid':os.getpid()}))
mode=(p/'mode').read_text()
if mode=='eof': sys.exit(0)
if mode=='silence':
    for _ in range(3050): sys.stdout.buffer.write(bytes(640));sys.stdout.buffer.flush()
    time.sleep(10)
else:
    pcm=(p/'speech.pcm').read_bytes()
    for i in range(0,len(pcm),640):
        sys.stdout.buffer.write(pcm[i:i+640]);sys.stdout.buffer.flush();time.sleep(.02)
    while True:
        sys.stdout.buffer.write(bytes(640));sys.stdout.buffer.flush();time.sleep(.02)
''')
    fake.chmod(0o700)
    env=dict(os.environ, PATH=str(work)+os.pathsep+os.environ['PATH'], CAPTIONS_FAKE_DIR=str(work), PIPEWIRE_RUNTIME_DIR=str(work), PIPEWIRE_REMOTE='no-real-audio-server')
    daemon=subprocess.Popen([binary,'daemon'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    def status():
        p=subprocess.run([binary,'status'],capture_output=True,text=True)
        return json.loads(p.stdout) if p.returncode==0 else None
    def idle():
        s=status()
        return s if s and s['state']=='idle' and not s['capture_active'] else None
    def stopped_pid(pid):
        try: os.kill(pid,0);return False
        except ProcessLookupError:return True
    def start(mode='speech',source='monitor'):
        (work/'mode').write_text(mode)
        (work/'capture.json').unlink(missing_ok=True)
        p=subprocess.Popen([binary,'start','--allow-calls','--silence-minutes','1','--source',source],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        wait_until(lambda:(work/'capture.json').exists())
        capture=json.loads((work/'capture.json').read_text())
        return p,capture
    try:
        wait_until(status)
        controller, capture=start()
        wait_until(lambda: (status() or {}).get('text'))
        assert 'stream.capture.sink = true' in ' '.join(capture['args'])
        subprocess.run([binary,'stop'],check=True)
        s=wait_until(idle);assert not s['text']
        wait_until(lambda:stopped_pid(capture['pid']))
        controller.wait(timeout=3)
        controller,capture=start(source='microphone')
        assert 'stream.capture.sink' not in ' '.join(capture['args'])
        controller.kill();controller.wait()
        s=wait_until(idle);assert 'Controller disconnected' in s['message']
        wait_until(lambda:stopped_pid(capture['pid']))
        controller,capture=start('silence')
        s=wait_until(idle);assert not s['text']
        controller.wait(timeout=3)
        wait_until(lambda:stopped_pid(capture['pid']))
        controller,capture=start('eof')
        s=wait_until(idle);assert not s['text'];assert s['message']!='Stopped; audio and text discarded'
        controller.wait(timeout=3)
        assert not list(work.glob('*.txt'))
        print('PASS: real-worker rolling text; monitor/mic arguments; Stop; owner crash; 60 s synthetic silence timeout; capture EOF; no transcript files')
    finally:
        daemon.terminate()
        try: daemon.wait(timeout=3)
        except subprocess.TimeoutExpired: daemon.kill();daemon.wait()
