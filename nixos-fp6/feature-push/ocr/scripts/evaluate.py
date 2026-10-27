#!/usr/bin/env python3
"""Compare cold process runs with one compute thread on synthetic fixtures."""
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time
import unicodedata

def normalized(text):
    return " ".join(unicodedata.normalize("NFC", text).split())

def distance(a, b):
    row = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        next_row = [i]
        for j, y in enumerate(b, 1):
            next_row.append(min(next_row[-1] + 1, row[j] + 1, row[j-1] + (x != y)))
        row = next_row
    return row[-1]

env = dict(os.environ, OMP_THREAD_LIMIT="1", RTEN_NUM_THREADS="1", RAYON_NUM_THREADS="1")
load_before = os.getloadavg()
rows = []
manifest = json.loads(Path("evaluation/generated/manifest.json").read_text())
for case in manifest:
    for engine in ["tesseract", "ocrs"]:
        # Both include process/model startup and input decoding. Tesseract uses the production preprocessing path.
        command = ([os.environ.get("OCR_TEST_BINARY", "target/release/fairphone-ocr"), "recognize", case["image"], "--lang", case["language"], "--json"] if engine == "tesseract" else ["target/release/examples/evaluate_ocrs", case["image"]])
        timings, rss, cpu = [], [], []
        for _ in range(3):
            with tempfile.NamedTemporaryFile() as memory:
                start = time.perf_counter()
                process = subprocess.run(["time", "-f", "%M %U %S", "-o", memory.name] + command, env=env, capture_output=True, text=True, check=True)
                timings.append((time.perf_counter() - start) * 1000)
                peak, user, system = Path(memory.name).read_text().split()
                rss.append(int(peak))
                cpu.append((float(user) + float(system)) * 1000)
                result = json.loads(process.stdout)
        expected, actual = normalized(case["text"]), normalized(result["text"])
        rows.append({"case":case["name"], "engine":engine, "expected":expected, "actual":actual,
                     "cer":distance(expected, actual)/len(expected),
                     "wer":distance(expected.split(), actual.split())/len(expected.split()),
                     "median_ms":round(statistics.median(timings), 1), "peak_rss_kib":max(rss), "median_cpu_ms":statistics.median(cpu), "runs_ms":timings})
        print(case["name"], engine, rows[-1]["cer"], rows[-1]["median_ms"], flush=True)
        if case["name"] == "romanian" and engine == "tesseract":
            # The preview uses a generated fixture, never real photos.
            preview = dict(result, path=case["image"], image_url=case["image"])
            Path("evaluation/preview-result.json").write_text(json.dumps(preview, ensure_ascii=False, indent=2))
report = {"platform":platform.platform(), "load_average_before":load_before, "load_average_after":os.getloadavg(), "font":"DejaVu Sans 2.37, 38 px", "ocrs_rustc":"1.95.0", "tesseract":subprocess.check_output(["tesseract", "--version"], text=True).splitlines()[0], "ocrs":"0.13.1", "rten":"0.26.0", "threads":1, "repeats":3, "results":rows}
Path("evaluation/results.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
