#!/usr/bin/env python3
"""Download two upstream Piper TTS voices for locally synthesized test data only."""
import concurrent.futures, hashlib, json, pathlib, urllib.request
root=pathlib.Path(__file__).resolve().parent.parent
work=root/'benchmarks/work/voices'; work.mkdir(parents=True,exist_ok=True)
voices={'en':'en/en_US/lessac/medium/en_US-lessac-medium','ro':'ro/ro_RO/mihai/medium/ro_RO-mihai-medium'}
def fetch(item):
    language,path=item; rows=[]
    for suffix in ['.onnx','.onnx.json']:
        url='https://huggingface.co/rhasspy/piper-voices/resolve/main/'+path+suffix
        target=work/(language+suffix)
        urllib.request.urlretrieve(url,target)
        rows.append(dict(language=language,url=url,sha256=hashlib.file_digest(target.open('rb'),'sha256').hexdigest()))
    return rows
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
    records=[row for rows in pool.map(fetch,voices.items()) for row in rows]
(root/'benchmarks/piper-sources.json').write_text(json.dumps(records,indent=2)+'\n')
print('Piper voice sources downloaded; no supplied recordings used')
