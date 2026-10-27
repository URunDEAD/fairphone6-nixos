#!/usr/bin/env python3
"""Local espeak synthetic speech; never downloads or records. Artifacts under /tmp."""
import json, os, pathlib, subprocess, tempfile
root = pathlib.Path(__file__).resolve().parent.parent
binary = pathlib.Path(os.environ.get('CAPTIONS_BIN', '/tmp/fp6-captions-target/debug/fp6-captions'))
results = []
with tempfile.TemporaryDirectory(prefix='fp6-captions-tts-') as directory:
    work = pathlib.Path(directory)
    for row in json.loads((root / 'benchmarks/corpus.json').read_text()):
        raw, wav = work / 'speech.wav', work / 'pcm.wav'
        subprocess.run(['espeak-ng', '-v', row['language'], '-s', '145', '-w', str(raw), row['text']], check=True)
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', str(raw), '-ar', '16000', '-ac', '1', '-c:a', 'pcm_s16le', str(wav)], check=True)
        cmd = [str(binary), 'benchmark', str(wav), '--language', row['language']]
        if os.environ.get('SEGMENTATION_ONLY') == '1':
            cmd += ['--segmentation-only']
        data = json.loads(subprocess.check_output(cmd, text=True))
        assert len(data['chunks']) >= 2, 'TTS should trigger several streaming windows'
        assert all(c['window_ms'] <= 1500 for c in data['chunks'])
        assert data['chunks'][0]['end_audio_ms'] <= 1800
        if data['kind'] == 'offline-decoder':
            assert sum(c['word_count'] for c in data['chunks']) > 0, 'Decoder returned no words for synthetic speech'
        data['id'] = row['id']
        results.append(data)
name = 'segmentation.json' if os.environ.get('SEGMENTATION_ONLY') == '1' else 'latency.json'
(root / 'benchmarks' / name).write_text(json.dumps(results, indent=2) + '\n')
print(f'Passed English/Romanian synthetic segmentation; saved benchmarks/{name}')
