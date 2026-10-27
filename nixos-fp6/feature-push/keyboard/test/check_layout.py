"""Static checks for maliit-keyboard layout QML files (Romanian focus).

usage: python3 check_layout.py [--self-test] FILE.qml...

Romanian layouts (Keyboard_ro*.qml): for every CharKey, extended and
extendedShifted have the same length and the shifted entries are the
capitals of the normal ones; no cedilla look-alikes. Other layouts are only
parsed (upstream has intentional asymmetries such as ß). The main ro layout must provide ă â î ș ț
(and capitals) as the long-press default of a/i/s/t, and the dictation slot.
"""
import re
import sys

CEDILLA = {"ş": "ş", "ţ": "ţ", "Ş": "Ş", "Ţ": "Ţ"}
RO_LETTERS = ["ă", "â", "î", "ș", "ț"]

KEY_RE = re.compile(r"CharKey\s*\{([^{}]*)\}")
LIST_RE = r"%s\s*:\s*\[([^\]]*)\]"
STR_RE = re.compile(r'"((?:[^"\\]|\\.)*)"')


def strings(s):
    return [bytes(m, "utf-8").decode("unicode_escape").encode("latin-1").decode("utf-8")
            if "\\" in m else m for m in STR_RE.findall(s)]


def keys(text):
    out = []
    for body in KEY_RE.findall(text):
        k = {}
        m = re.search(r'label\s*:\s*"((?:[^"\\]|\\.)*)"', body)
        k["label"] = m.group(1) if m else None
        for name in ("extended", "extendedShifted"):
            m = re.search(LIST_RE % name + r"\s*[;}\n]", body + "\n")
            k[name] = strings(m.group(1)) if m else None
        out.append(k)
    return out


def check(path, text):
    errors = []
    is_ro = "/Keyboard_ro" in path or path.startswith("Keyboard_ro")
    if not keys(text):
        errors.append(f"{path}: no CharKey found")
    for k in keys(text) if is_ro else []:
        e, es = k["extended"], k["extendedShifted"]
        if e is None and es is None:
            continue
        if (e is None) != (es is None) or len(e) != len(es):
            errors.append(f"{path}: key {k['label']!r}: extended/extendedShifted differ in length")
            continue
        for a, b in zip(e, es):
            if a.isalpha() and a.upper() != b and a != b:
                errors.append(f"{path}: key {k['label']!r}: shifted {b!r} is not the capital of {a!r}")
    if is_ro:
        for c, name in CEDILLA.items():
            if c in text:
                errors.append(f"{path}: contains cedilla {name} (U+{ord(c):04X}); use comma below")
    if re.search(r"/Keyboard_ro\.qml$", path) or path == "Keyboard_ro.qml":
        allk = keys(text)
        ext = {x for k in allk for x in (k["extended"] or [])}
        exts = {x for k in allk for x in (k["extendedShifted"] or [])}
        for c in RO_LETTERS:
            if c not in ext:
                errors.append(f"{path}: {c} missing from long-press popups")
            if c.upper() not in exts:
                errors.append(f"{path}: {c.upper()} missing from shifted long-press popups")
        # The first popup entry is the one highlighted on long-press.
        for label, want in (("t", "ț"), ("s", "ș"), ("a", "ă"), ("i", "î")):
            k = [k for k in allk if k["label"] == label]
            if not k or not k[0]["extended"] or k[0]["extended"][0] != want:
                errors.append(f"{path}: long-press on {label} should default to {want}")
        if "MicKey" not in text:
            errors.append(f"{path}: dictation slot (MicKey) missing")
    return errors


def self_test():
    good = ('CharKey { label: "s"; shifted: "S"; extended: ["ș","$"]; extendedShifted: ["Ș", "$"] }\n'
            'CharKey { label: "t"; shifted: "T"; extended: ["ț", "5"]; extendedShifted: ["Ț", "5"] }\n'
            'CharKey { label: "a"; shifted: "A"; extended: ["ă", "â"]; extendedShifted: ["Ă", "Â"] }\n'
            'CharKey { label: "i"; shifted: "I"; extended: ["î"]; extendedShifted: ["Î"] }\nMicKey {}\n')
    assert check("x/Keyboard_ro.qml", good) == [], check("x/Keyboard_ro.qml", good)
    bad = good.replace("ș", "ş")
    errs = check("x/Keyboard_ro.qml", bad)
    assert any("cedilla" in e for e in errs) and any("ș missing" in e for e in errs), errs
    errs = check("x/Keyboard_ro.qml", good.replace('["Ș", "$"]', '["$"]'))
    assert any("length" in e for e in errs), errs
    errs = check("x/Keyboard_ro.qml", good.replace('"Ă", "Â"', '"Â", "Ă"'))
    assert any("capital" in e for e in errs), errs
    errs = check("x/Keyboard_ro.qml", good.replace('["ț", "5"]', '["5", "ț"]').replace('["Ț", "5"]', '["5", "Ț"]'))
    assert any("default to ț" in e for e in errs), errs
    assert check("x/Keyboard_en.qml", 'CharKey { label: "s"; extended: ["ş"]; extendedShifted: ["Ş"] }') == []
    assert strings(r'"\"" "a"') == ['"', "a"]
    print("check_layout self-test: ok")


def main(argv):
    if argv and argv[0] == "--self-test":
        self_test()
        argv = argv[1:]
    errors = []
    for p in argv:
        with open(p, encoding="utf-8") as f:
            errors += check(p, f.read())
    for e in errors:
        print(e, file=sys.stderr)
    if errors:
        return 1
    if argv:
        print(f"check_layout: {len(argv)} file(s) ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
