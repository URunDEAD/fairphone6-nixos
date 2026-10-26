#!/usr/bin/env python3
"""Camera fast-launch timing from measure.sh logs (laptop side).

analyze.py DIR [DIR ...]  ->  one row per run, times in ms after the launch
request (lomiri-app-launch = what a launcher tap does).

Columns:
  unit     systemd "Started" of the app unit (process start)
  main     FLMARK main (the binary's static init; new builds only)
  engine   QML engine load starts (FLMARK engine-load / "Import path added")
  root     camera-app.qml completed (FLMARK root-completed)
  camopen  fp6libcamerasrc built (camera opening: CameraManager, IPA)
  overlay  viewfinder overlay QML ready (FLMARK overlay-ready)
  frame    first frame out of libcamerasrc ("first frame")
  shown    first buffer swap of the app window after that frame
  ae       first stats frame from which the AGC stays put for 3 frames in a
           row (|log2 factor| < 0.08, or exposure and gain unchanged at a
           limit) = exposure settled
  aeN      stats frames from the first one until settled
  exp0     the AGC's first exposure (lines) and gain: what the sensor started with
  expN     the last ones of the run (the settled state)
"""
import math
import re
import sys
from pathlib import Path

DEADBAND = 0.08


def steady(factor, eg):
    """The AGC leaves the exposure alone: inside its deadband, or at a limit
    (exposure and gain written back unchanged, e.g. a dark scene at the
    longest exposure and highest gain)."""
    if abs(math.log2(max(factor, 1e-6))) < DEADBAND:
        return True
    return bool(eg) and eg[0] == eg[1] and eg[2] == eg[3]


def parse(path):
    t0 = None
    ev = {}
    swaps = []
    ae = []
    for line in path.read_text(errors='replace').splitlines():
        m = re.match(r'FLMARK launch ([\d.]+)', line)
        if m:
            t0 = float(m.group(1))
            continue
        m = re.match(r'([\d.]+) \S+ (\S+?)\[\d+\]: (.*)', line)
        if not m:
            continue
        ts, msg = float(m.group(1)), m.group(3)
        fm = re.match(r'FLMARK (\S+) ([\d.]+)', msg)
        if fm:
            name, t = fm.group(1), float(fm.group(2))
            if name == 'swap':
                swaps.append(t)
            else:
                ev.setdefault(name, t)
            continue
        if 'Import path added' in msg:
            ev.setdefault('import', ts)
        elif 'lsrc_build' in msg:
            ev.setdefault('camopen', ts)
        elif 'first frame' in msg:
            ev.setdefault('frame', ts)
        elif 'IPASoftExposure' in msg and 'factor' in msg:
            f = re.search(r'factor ([\d.eE+-]+)', msg)
            e = re.search(r' exp (\d+)->(\d+) again ([\d.]+)->([\d.]+)', msg)
            if f:
                ae.append((ts, float(f.group(1)), e.groups() if e else None))
    sysf = path.with_suffix('.sys')
    if sysf.exists():
        for line in sysf.read_text(errors='replace').splitlines():
            m = re.match(r'([\d.]+) .*Started lomiri-app-launch--application-legacy--lomiri-camera-app', line)
            if m:
                ev.setdefault('unit', float(m.group(1)))
    out = {}
    if t0 is None:
        return out
    rel = lambda t: None if t is None else round((t - t0) * 1000)
    out['unit'] = rel(ev.get('unit'))
    out['main'] = rel(ev.get('main'))
    out['engine'] = rel(ev.get('engine-load', ev.get('import')))
    out['root'] = rel(ev.get('root-completed'))
    out['camopen'] = rel(ev.get('camopen'))
    out['overlay'] = rel(ev.get('overlay-ready'))
    out['frame'] = rel(ev.get('frame'))
    if ev.get('frame') and swaps:
        later = [s for s in swaps if s >= ev['frame']]
        out['shown'] = rel(later[0]) if later else None
    settled = None
    for i in range(len(ae)):
        win = ae[i:i + 3]
        if len(win) == 3 and all(steady(f, e) for _, f, e in win):
            settled = i
            break
    out['ae'] = rel(ae[settled][0]) if settled is not None else None
    out['aeN'] = settled
    if ae and ae[0][2]:
        out['exp0'] = '%s/%s' % (ae[0][2][0], ae[0][2][2])
        out['expN'] = '%s/%s' % (ae[-1][2][1], ae[-1][2][3])
    return out


COLS = ['unit', 'main', 'engine', 'root', 'camopen', 'overlay', 'frame', 'shown', 'ae', 'aeN', 'exp0', 'expN']


def main():
    print('| run | ' + ' | '.join(COLS) + ' |')
    print('|---' * (len(COLS) + 1) + '|')
    for d in sys.argv[1:]:
        for log in sorted(Path(d).glob('run*.log'), key=lambda p: int(re.search(r'run(\d+)', p.name).group(1))):
            r = parse(log)
            print('| %s/%s | ' % (Path(d).name, log.stem) + ' | '.join(
                '' if r.get(c) is None else str(r.get(c)) for c in COLS) + ' |')


if __name__ == '__main__':
    main()
