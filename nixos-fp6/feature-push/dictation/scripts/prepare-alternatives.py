#!/usr/bin/env python3
"""Explicit source/model download for evaluation; never runs in the daemon."""
import hashlib, io, json, pathlib, tarfile, urllib.request, zipfile
root=pathlib.Path(__file__).resolve().parent.parent
work=root/'benchmarks/work'; work.mkdir(parents=True,exist_ok=True)

def fetch(url):
    with urllib.request.urlopen(url,timeout=120) as reply: return reply.read()

metadata=json.loads(fetch('https://pypi.org/pypi/vosk/0.3.45/json'))
wheel=next(x for x in metadata['urls'] if 'manylinux' in x['filename'] and 'x86_64' in x['filename'])
blob=fetch(wheel['url'])
assert hashlib.sha256(blob).hexdigest()==wheel['digests']['sha256']
with zipfile.ZipFile(io.BytesIO(blob)) as archive:
    destination=work/'vosk/libvosk.so'; destination.parent.mkdir(exist_ok=True)
    destination.write_bytes(archive.read('vosk/libvosk.so'))
model_url='https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip'
model=fetch(model_url)
with zipfile.ZipFile(io.BytesIO(model)) as archive:
    for name in archive.namelist():
        p=pathlib.PurePosixPath(name)
        if p.is_absolute() or '..' in p.parts: raise ValueError('Unsafe archive path')
    archive.extractall(work)
moon_url='https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27.tar.bz2'
moon=fetch(moon_url)
with tarfile.open(fileobj=io.BytesIO(moon),mode='r:bz2') as archive:
    # Ignore supplied example recordings: all evaluation audio is synthesized locally.
    for member in archive.getmembers():
        if member.isfile() and pathlib.PurePosixPath(member.name).name in ['encoder_model.ort','decoder_model_merged.ort','tokens.txt','LICENSE']:
            archive.extract(member,work,filter='data')
sources=dict(vosk_wheel_url=wheel['url'],vosk_wheel_sha256=wheel['digests']['sha256'],vosk_model_url=model_url,vosk_model_sha256=hashlib.sha256(model).hexdigest(),moonshine_url=moon_url,moonshine_sha256=hashlib.sha256(moon).hexdigest())
(root/'benchmarks/alternative-sources.json').write_text(json.dumps(sources,indent=2)+'\n')
