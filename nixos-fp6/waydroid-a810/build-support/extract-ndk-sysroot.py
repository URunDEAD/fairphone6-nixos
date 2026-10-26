#!/usr/bin/env python3
"""Extract data files needed by native Clang; never run an x86 NDK binary."""
from pathlib import Path
import argparse
import hashlib
import json
import zipfile

root = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--archive', type=Path, default=root / 'toolchain/android-ndk-r27d-linux.zip')
parser.add_argument('--destination', type=Path, default=root / 'toolchain')
args = parser.parse_args()
archive = args.archive
digests = {name: hashlib.new(name) for name in ["sha1", "sha256"]}
with archive.open("rb") as stream:
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        for digest in digests.values():
            digest.update(block)
hashes = {name: digest.hexdigest() for name, digest in digests.items()}
if (hashes["sha1"] != "22105e410cf29afcf163760cc95522b9fb981121" or
    hashes["sha256"] != "601246087a682d1944e1e16dd85bc6e49560fe8b6d61255be2829178c8ed15d9"):
    raise SystemExit("NDK r27d archive checksum mismatch")
prefix = "android-ndk-r27d/toolchains/llvm/prebuilt/linux-x86_64/"
keep = [prefix + part for part in [
    "sysroot/usr/include/", "sysroot/usr/lib/aarch64-linux-android/",
    "lib/clang/18/include/", "lib/clang/18/lib/linux/",
    "clang_source_info.md",
]] + ["android-ndk-r27d/source.properties", "android-ndk-r27d/meta/abis.json"]
count = 0
with zipfile.ZipFile(archive) as source:
    for member in source.infolist():
        if any(member.filename.startswith(p) for p in keep):
            if ".." in Path(member.filename).parts:
                raise SystemExit("Invalid archive path")
            source.extract(member, args.destination)
            count += 1
report = {"archive": archive.name, **hashes, "extracted_entries": count,
          "included": keep, "ndk_x86_executables_extracted": False}
(args.destination / "ndk-input.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
