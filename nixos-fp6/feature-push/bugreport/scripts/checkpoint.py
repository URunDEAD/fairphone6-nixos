#!/usr/bin/env python3
"""Keep reviewable commits in /tmp when the workspace .git is read-only."""
import pathlib, shutil, subprocess, sys
root = pathlib.Path(__file__).resolve().parents[1]
history = pathlib.Path('/tmp/fp6-bugreport-history')
if not (history / '.git').is_dir():
    history.mkdir(parents=True, exist_ok=True)
    subprocess.run(['git', 'init', str(history)], check=True)
    subprocess.run(['git', '-C', str(history), 'config', 'user.name', 'Codex'], check=True)
    subprocess.run(['git', '-C', str(history), 'config', 'user.email', 'codex@localhost'], check=True)
    shutil.copy2(root / 'TASK.md', history / 'TASK.md')
    subprocess.run(['git', '-C', str(history), 'add', 'TASK.md'], check=True)
    subprocess.run(['git', '-C', str(history), 'commit', '-m', 'Task brief'], check=True)
exclude = {'.git', '.agents', '.aws', '.codex', 'target', 'patches', 'result', '__pycache__'}
for item in ([root / p for p in sys.argv[2:]] if len(sys.argv) > 2 else root.iterdir()):
    if item.name in exclude or item.name.startswith('result'):
        continue
    dest = history / item.relative_to(root)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if item.is_dir():
        if dest.exists(): shutil.rmtree(dest)
        shutil.copytree(item, dest)
    else: shutil.copy2(item, dest)
subprocess.run(['git', '-C', str(history), 'add', '-A'], check=True)
subprocess.run(['git', '-C', str(history), 'commit', '-m', sys.argv[1]], check=True)
patches = root / 'patches'
patches.mkdir(exist_ok=True)
for path in patches.glob('*.patch'): path.unlink()
subprocess.run(['git', '-C', str(history), 'format-patch', '--binary', '--no-signature', '-o', str(patches), 'HEAD~' + str(int(subprocess.check_output(['git', '-C', str(history), 'rev-list', '--count', 'HEAD'])) - 1) + '..HEAD'], check=True)
(patches / 'series').write_text(''.join(p.name + '\n' for p in sorted(patches.glob('*.patch'))))
