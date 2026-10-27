#!/usr/bin/env python3
"""Synthetic-only benchmark. Requires nix-shell, installed models, and release build."""
import json, os, pathlib, platform, re, subprocess, sys, wave
ROOT = pathlib.Path(__file__).resolve().parent.parent
WORK = ROOT / 'benchmarks/work'
WORK.mkdir(parents=True, exist_ok=True)
tts = os.environ.get('FP6_TTS', 'espeak-ng')
result_file = os.environ.get('FP6_BENCH_RESULTS', 'results.json')
corpus = json.loads((ROOT / 'benchmarks/corpus.json').read_text())

def words(text):
    return re.findall(r"[^\W_]+(?:['’][^\W_]+)?", text.casefold(), re.UNICODE)

def wer(reference, hypothesis):
    a, b = words(reference), words(hypothesis)
    prev = list(range(len(b) + 1))
    for i, aw in enumerate(a, 1):
        row = [i]
        for j, bw in enumerate(b, 1):
            row.append(min(row[-1] + 1, prev[j] + 1, prev[j-1] + (aw != bw)))
        prev = row
    return prev[-1] / len(a)

def main():
    for clip in corpus:
        source = WORK / (clip['id'] + '-tts.wav')
        wav = WORK / (clip['id'] + '.wav')
        if tts == 'piper':
            subprocess.run(['piper', '--model', str(WORK/'voices'/(clip['language']+'.onnx')), '--output_file', str(source)], input=clip['text'], text=True, check=True)
        else:
            subprocess.run(['espeak-ng', '-v', clip['language'], '-s', '145', '-w', str(source), clip['text']], check=True)
        subprocess.run(['ffmpeg', '-nostdin', '-v', 'error', '-y', '-i', str(source), '-ar', '16000', '-ac', '1', '-c:a', 'pcm_s16le', str(wav)], check=True)
    
    if "--generate-only" in sys.argv:
        return
    results = []
    for model in ['tiny-q5_1', 'base-q5_1', 'small-q5_1']:
        for clip in corpus:
            wav = WORK / (clip['id'] + '.wav')
            with wave.open(str(wav)) as audio:
                seconds = audio.getnframes() / audio.getframerate()
            stat = WORK / 'timing.txt'
            command = [os.environ.get('FP6_BENCH_BINARY', str(ROOT / 'target/release/fp6-dictate')), 'transcribe', str(wav), '--language', clip['language'], '--model', model]
            load = os.getloadavg()
            run = subprocess.run(['time', '-f', '%e %M', '-o', str(stat), *command], capture_output=True, text=True, env={**os.environ, 'XDG_DATA_HOME': str(WORK / 'data')}, check=True)
            wall, rss = map(float, stat.read_text().split())
            result = dict(load_average=load, model=model, clip=clip['id'], language=clip['language'], audio_seconds=round(seconds,3), wall_seconds=wall, real_time_factor=round(wall/seconds,3), peak_rss_mib=round(rss/1024,1), wer=round(wer(clip['text'],run.stdout),3), reference=clip['text'], hypothesis=run.stdout.strip())
            results.append(result)
            print(json.dumps(result, ensure_ascii=False), flush=True)
            (ROOT/'benchmarks'/result_file).write_text(json.dumps(dict(platform=platform.platform(), build=os.environ.get('FP6_BENCH_BUILD', 'portable'), threads=8, tts=tts + ', resampled to mono PCM16 16kHz', runs=results), ensure_ascii=False, indent=2)+'\n')

if __name__ == "__main__":
    main()
