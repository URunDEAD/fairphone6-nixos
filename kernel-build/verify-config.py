"""Fail if required features or any original FP6 driver disappear."""
from pathlib import Path
import sys

def parse(path):
    return dict(line.split('=', 1) for line in Path(path).read_text().splitlines()
                if line.startswith('CONFIG_') and '=' in line)

here = Path(__file__).resolve().parent
actual = parse(sys.argv[1])
base = parse(here.parent / 'nixos-fp6/assets/kernel.config')
features = parse(here / 'features.config')
# The original Alpine kernel used Clang 23.1.3. Pinned nixpkgs uses 21.1.8;
# these capability probes reflect the actual compiler and cannot be forced.
toolchain_probes = {
    'CONFIG_CC_HAS_COUNTED_BY_PTR', 'CONFIG_CC_HAS_ALLOC_TOKEN',
    'CONFIG_WARN_CONTEXT_ANALYSIS',
    'CONFIG_OPENSSL_SUPPORTS_ML_DSA', 'CONFIG_PAHOLE_HAS_BTF_TAG',
    'CONFIG_PAHOLE_HAS_LANG_EXCLUDE',
}
for name, value in features.items():
    assert actual.get(name) == value, (name, value, actual.get(name))
for name, value in base.items():
    if value in ('y', 'm') and name not in features and name not in toolchain_probes:
        assert actual.get(name) == value, ('original enabled feature changed', name, value, actual.get(name))
print('Required additions and all original enabled non-toolchain kernel features verified.')
for name in sorted(toolchain_probes):
    if actual.get(name) != base.get(name):
        print('Toolchain probe changed:', name, base.get(name), '->', actual.get(name))
