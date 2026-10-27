#!/usr/bin/env python3
"""Evaluation only: run Vosk and 2026 Moonshine against the synthetic corpus."""
import ctypes as c
import json, os, pathlib, subprocess, sys, wave
from benchmark import ROOT, WORK, corpus, wer

def vosk(wav):
    lib=c.CDLL(str(WORK/'vosk/libvosk.so'))
    lib.vosk_set_log_level.argtypes=[c.c_int]; lib.vosk_set_log_level(-1)
    lib.vosk_model_new.argtypes=[c.c_char_p]; lib.vosk_model_new.restype=c.c_void_p
    lib.vosk_recognizer_new.argtypes=[c.c_void_p,c.c_float]; lib.vosk_recognizer_new.restype=c.c_void_p
    lib.vosk_recognizer_accept_waveform.argtypes=[c.c_void_p,c.c_char_p,c.c_int]; lib.vosk_recognizer_accept_waveform.restype=c.c_int
    for name in ['vosk_recognizer_result','vosk_recognizer_final_result']:
        getattr(lib,name).argtypes=[c.c_void_p]; getattr(lib,name).restype=c.c_char_p
    lib.vosk_recognizer_free.argtypes=[c.c_void_p]; lib.vosk_model_free.argtypes=[c.c_void_p]
    model=lib.vosk_model_new(os.fsencode(WORK/'vosk-model-small-en-us-0.15'))
    if not model: raise RuntimeError('Vosk model load failed')
    recognizer=lib.vosk_recognizer_new(model,16000)
    if not recognizer: raise RuntimeError('Vosk recognizer failed')
    text=[]
    with wave.open(wav) as audio:
        while data:=audio.readframes(4000):
            result=lib.vosk_recognizer_accept_waveform(recognizer,data,len(data))
            if result<0: raise RuntimeError('Vosk decode failed')
            if result: text.append(json.loads(lib.vosk_recognizer_result(recognizer))['text'])
    text.append(json.loads(lib.vosk_recognizer_final_result(recognizer))['text'])
    lib.vosk_recognizer_free(recognizer); lib.vosk_model_free(model)
    print(' '.join(text).strip())

def main():
    results=[]
    moon=WORK/'sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27'
    for engine in ['vosk-small-en-us-0.15','moonshine-tiny-en-v2-2026-02-27']:
        for clip in [x for x in corpus if x['language']=='en']:
            wav=WORK/(clip['id']+'.wav')
            with wave.open(str(wav)) as audio: seconds=audio.getnframes()/audio.getframerate()
            if engine.startswith('vosk'):
                command=[sys.executable,__file__,'--vosk',str(wav)]
            else:
                command=['sherpa-onnx-offline','--num-threads=8','--provider=cpu',f'--moonshine-encoder={moon}/encoder_model.ort',f'--moonshine-merged-decoder={moon}/decoder_model_merged.ort',f'--tokens={moon}/tokens.txt',str(wav)]
            stat=WORK/'alternative-time.txt'
            load=os.getloadavg()
            run=subprocess.run(['time','-f','%e %M','-o',str(stat),*command],capture_output=True,text=True,check=True)
            hypothesis=run.stdout.strip()
            if engine.startswith('moonshine'):
                lines=(run.stdout+'\n'+run.stderr).splitlines()
                hypothesis=next(json.loads(line)['text'].strip() for line in lines if line.startswith('{') and '"text"' in line)
            wall,rss=map(float,stat.read_text().split())
            row=dict(load_average=load,engine=engine,clip=clip['id'],language='en',audio_seconds=round(seconds,3),wall_seconds=wall,real_time_factor=round(wall/seconds,3),peak_rss_mib=round(rss/1024,1),wer=round(wer(clip['text'],hypothesis),3),reference=clip['text'],hypothesis=hypothesis)
            results.append(row); print(json.dumps(row),flush=True)
    (ROOT/'benchmarks'/os.environ.get('FP6_ALT_RESULTS','alternatives.json')).write_text(json.dumps(results,indent=2)+'\n')

if __name__=='__main__':
    if len(sys.argv)>1 and sys.argv[1]=='--vosk': vosk(sys.argv[2])
    else: main()
