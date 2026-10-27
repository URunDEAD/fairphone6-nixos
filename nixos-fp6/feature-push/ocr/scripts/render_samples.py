#!/usr/bin/env python3
"""Render synthetic fixtures only: no user screenshots or photos are read."""
import json
import os
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont, ImageFilter

out = Path("evaluation/generated")
out.mkdir(parents=True, exist_ok=True)
font_path = os.environ.get("OCR_TEST_FONT", "/run/current-system/sw/share/fonts/truetype/DejaVuSans.ttf")
if not Path(font_path).exists():
    import glob
    font_path = sorted(glob.glob("/nix/store/*-dejavu-fonts-*/share/fonts/truetype/DejaVuSans.ttf"))[0]
font = ImageFont.truetype(font_path, 38)
cases = [
    ("english", ["A quiet morning in the city.", "Read this screenshot without a network.", "Meeting at 10:30. Bring 2 notebooks."], "eng"),
    ("romanian", ["În România, învățăm în fiecare zi.", "Ștefan și Ana cumpără pâine și mere.", "Țara are munți, păduri și râuri."], "ron"),
    ("mixed", ["Offline text recognition", "București: știri și călătorii", "English + Română: ă â î ș ț"], "eng+ron"),
    ("photo", ["Bon de cumpărături", "Pâine și brânză: 24 lei", "Mulțumim! O zi frumoasă!"], "ron"),
]
manifest = []
for name, lines, lang in cases:
    image = Image.new("RGB", (1100, 320), "#fcfcfa")
    draw = ImageDraw.Draw(image)
    for i, line in enumerate(lines):
        draw.text((45, 40 + i * 80), line, fill="#20282c", font=font)
    if name == "photo":
        image = image.rotate(2, resample=Image.Resampling.BICUBIC, expand=True, fillcolor="#dedbd4").filter(ImageFilter.GaussianBlur(.45))
        image.save(out / (name + ".jpg"), quality=85)
        filename = name + ".jpg"
    else:
        filename = name + ".png"
        image.save(out / filename)
    manifest.append({"name": name, "image": str(out / filename), "text": "\n".join(lines), "language": lang})
(out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2))
print(f"Rendered {len(cases)} synthetic fixtures with {font_path}")
