#!/usr/bin/env python3
"""Explicit online model-download check, followed by a fake corrupted download.
Requires a release CLI and curl. Does not capture audio or use normal user data.
"""
import hashlib, json, os, pathlib, shutil, subprocess, sys, tempfile
root=pathlib.Path(__file__).resolve().parent.parent
binary=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else root/'target/release/fp6-dictate').resolve()
model=next(m for m in json.loads((root/'models.json').read_text()) if m['id']=='tiny-q5_1')
with tempfile.TemporaryDirectory(prefix='dictation-download-') as temp:
    work=pathlib.Path(temp)
    env={**os.environ,'XDG_DATA_HOME':str(work/'data')}
    subprocess.run([str(binary),'download',model['id']],env=env,check=True)
    installed=work/'data/fp6-dictation/models'/model['filename']
    checksum=lambda: hashlib.file_digest(installed.open('rb'),'sha256').hexdigest()
    assert checksum()==model['sha256']
    fake=work/'bin'; fake.mkdir()
    curl=fake/'curl'
    curl.write_text('#!'+shutil.which('sh')+'\nwhile [ "$#" -gt 0 ]; do\n  if [ "$1" = --output ]; then shift; : > "$1"; exit 0; fi\n  shift\ndone\nexit 1\n')
    curl.chmod(0o700)
    failed=subprocess.run([str(binary),'download',model['id']],env={**env,'PATH':str(fake)+':'+env['PATH']},capture_output=True,text=True)
    assert failed.returncode!=0 and 'checksum or size mismatch' in failed.stderr, failed
    assert checksum()==model['sha256'], 'Corrupt replacement damaged installed model'
    assert not list(installed.parent.glob('*.part')), 'Partial file leaked'
print('Verified download, corrupt replacement rejection, old-model preservation, partial cleanup: passed')
