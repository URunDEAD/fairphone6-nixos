#!/bin/sh
# pw-record stand-in for the laptop preview: synthetic tone, ignores its arguments.
exec python3 "$(dirname "$0")/fake-signals.py" audio "${FP6_FAKE_TONE:-218.5}"
