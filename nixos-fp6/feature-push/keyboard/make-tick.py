# Writes the maliit-keyboard key press sound (16-bit mono PCM WAV, 44.1 kHz):
# a 14 ms decaying 1.9 kHz click. Upstream maliit-keyboard references
# sounds/key_tick2_quiet.wav in its GSettings default but ships no sound.
import math
import struct
import sys
import wave

RATE = 44100
N = int(RATE * 0.014)
frames = bytearray()
for i in range(N):
    t = i / RATE
    env = math.exp(-t * 420.0) * min(1.0, i / 24.0)
    s = 0.55 * env * math.sin(2 * math.pi * 1900.0 * t)
    frames += struct.pack("<h", int(round(s * 32767)))
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(RATE)
    w.writeframes(bytes(frames))
