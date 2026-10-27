#!/usr/bin/env python3
"""Real Whisper over private D-Bus with synthetic PCM; never opens a microphone.
Run after benchmark.py so the generated clip and its direct result exist.
"""
import json, os, pathlib, shutil, subprocess, tempfile, time, wave
root=pathlib.Path(__file__).resolve().parent.parent
language=os.environ.get('FP6_TEST_LANGUAGE','en')
assert language in ['en','ro']
clip=language+'-note'
results_file=os.environ.get('FP6_BENCH_RESULTS','results.json')
model_id=os.environ.get('FP6_TEST_MODEL','base-q5_1')
with tempfile.TemporaryDirectory(prefix='dictation-session-') as temp:
    temp=pathlib.Path(temp)
    config=temp/'bus.conf'
    config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow own="*"/><allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>')
    bus=subprocess.Popen(['dbus-daemon','--config-file',str(config),'--nofork','--print-address=1'],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    daemon=None
    try:
        address=bus.stdout.readline().strip()
        with wave.open(str(root/'benchmarks/work'/(clip+'.wav'))) as audio:
            (temp/'audio.raw').write_bytes(audio.readframes(audio.getnframes()))
        capture=temp/'capture'
        # Constant script; the path is passed in an environment variable, not interpolated shell code.
        capture.write_text('#!'+shutil.which('sh')+'\ncat "$FP6_SYNTHETIC_PCM"\nexec sleep 60\n')
        capture.chmod(0o700)
        env={**os.environ,'DBUS_SESSION_BUS_ADDRESS':address,'XDG_DATA_HOME':str(root/'benchmarks/work/data'),'XDG_CONFIG_HOME':str(temp/'config'),'FP6_CAPTURE_PROGRAM':str(capture),'FP6_SYNTHETIC_PCM':str(temp/'audio.raw')}
        preferences=temp/'config/fp6-dictation'
        preferences.mkdir(parents=True)
        (preferences/'settings.ini').write_text('[General]\nlanguage='+language+'\nmodel='+model_id+'\n')
        daemon=subprocess.Popen([str(root/'target/release/fp6-dictationd')],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        for _ in range(100):
            probe=subprocess.run(['dbus-send','--bus='+address,'--print-reply','--dest=org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus.NameHasOwner','string:com.fairphone6.Dictation1'],capture_output=True,text=True)
            if 'boolean true' in probe.stdout: break
            if daemon.poll() is not None: raise RuntimeError(daemon.communicate())
            time.sleep(0.1)
        else: raise RuntimeError('Daemon did not acquire its name')
        run=subprocess.run([str(root/'target/release/fp6-dictate'),'--seconds','1'],env=env,capture_output=True,text=True,check=True,timeout=110)
        expected=next(x['hypothesis'] for x in json.loads((root/'benchmarks'/results_file).read_text())['runs'] if x['clip']==clip and x['model']==model_id)
        assert run.stdout.strip()==expected and expected, (run.stdout,expected)
        daemon.terminate(); stdout,stderr=daemon.communicate(timeout=10)
        assert daemon.returncode==0 and stdout=='' and stderr=='', (stdout,stderr)
        print(language + ': Private D-Bus + synthetic capture + real Whisper worker matches direct transcription; no daemon transcript logging: passed')
    finally:
        if daemon and daemon.poll() is None: daemon.kill(); daemon.wait()
        bus.terminate(); bus.communicate(timeout=10)
