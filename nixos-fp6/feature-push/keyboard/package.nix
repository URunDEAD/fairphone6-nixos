# Word prediction models (and the key click) for maliit-keyboard on the FP6.
#
# fp6-kbd-ngram (./ngram, Rust, no dependencies) builds the "fp6-ngram 1" TSV
# models the patched maliit-keyboard loads (maliit-keyboard-ngram-prediction.patch).
# Corpora, all public domain or CC0 (results/feature-push-20261024/keyboard/README.md):
#  ro: Ion Creangă, "Amintiri din copilărie" (1881; author d. 1889), the text
#      maliit-keyboard itself used for its presage model until 2.3.1, fetched
#      from maliit/keyboard at the commit before the removal;
#      I. L. Caragiale, "Nuvele" (author d. 1912; Project Gutenberg #64597) and
#      Ioan Slavici, "Povești" (author d. 1925; Project Gutenberg #62916),
#      Gutenberg header/footer removed (corpus/*.txt, downloaded 2026-10-27);
#      corpus/ro-seed.txt, everyday sentences written for this repo (CC0), x3.
#  en: Oscar Wilde, "The Picture of Dorian Gray" (1890; author d. 1900), also
#      from maliit/keyboard; corpus/en-seed.txt (CC0), x3.
{ lib, stdenvNoCC, rustPlatform, fetchurl, python3 }:
let
  tool = rustPlatform.buildRustPackage {
    pname = "fp6-kbd-ngram";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./ngram;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./ngram/Cargo.lock;
    # Unit tests: Romanian normalisation (cedilla -> comma below), tokenizer,
    # counts, TSV round trip, context, backoff, diacritic-insensitive
    # matching, learning, keystroke evaluation.
    doCheck = true;
    meta.mainProgram = "fp6-kbd-ngram";
  };

  # Last maliit/keyboard commit that still shipped the presage training texts
  # (parent of 9b647844 "Remove text prediction feature for now").
  maliitRev = "0ea21b3408f41c5051432465c74e8d9771b26946";
  maliitText = path: hash: fetchurl {
    url = "https://raw.githubusercontent.com/maliit/keyboard/${maliitRev}/${path}";
    inherit hash;
  };
  creanga = maliitText "plugins/ro/src/amintiri_din_copilarie.txt"
    "sha256-6hvPGHufK0FsLP2L7IZh7sKt7rAds8qDmN4lhSgweR8=";
  dorian = maliitText "plugins/en/src/the_picture_of_dorian_gray.txt"
    "sha256-Flsviqpve4sQDuuDzQauJShCCU0Q7wcWYpg7JV4bRXk=";
in
stdenvNoCC.mkDerivation {
  pname = "fp6-keyboard-models";
  version = "1";
  src = ./corpus;
  nativeBuildInputs = [ tool python3 ];

  buildPhase = ''
    runHook preBuild
    fp6-kbd-ngram build --lang ro --out ngram_ro.tsv \
      --meta "source=Creangă, Amintiri din copilărie (maliit/keyboard ${maliitRev}); Caragiale, Nuvele (PG 64597); Slavici, Povești (PG 62916); ro-seed.txt (CC0, x3)" \
      --meta "licence=public domain texts + CC0 seed" \
      --corpus ${creanga} --corpus ro-caragiale-nuvele.txt --corpus ro-slavici-povesti.txt \
      --weight 3 --corpus ro-seed.txt
    fp6-kbd-ngram build --lang en --out ngram_en.tsv \
      --meta "source=Wilde, The Picture of Dorian Gray (maliit/keyboard ${maliitRev}); en-seed.txt (CC0, x3)" \
      --meta "licence=public domain text + CC0 seed" \
      --corpus ${dorian} --weight 3 --corpus en-seed.txt
    # Reference answers for the C++ predictor's parity test.
    fp6-kbd-ngram parity ngram_ro.tsv ro-seed.txt > parity_ro.tsv
    fp6-kbd-ngram parity ngram_en.tsv en-seed.txt > parity_en.tsv
    grep -q . parity_ro.tsv && grep -q . parity_en.tsv
    # The Romanian model must only contain comma-below ș/ț.
    if grep -q $'ş\|ţ\|Ş\|Ţ' ngram_ro.tsv; then
      echo "cedilla ş/ţ in ngram_ro.tsv" >&2; exit 1
    fi
    python3 ${./make-tick.py} key_tick.wav
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    install -Dm644 -t $out/share/fp6-keyboard ngram_ro.tsv ngram_en.tsv parity_ro.tsv parity_en.tsv key_tick.wav
    runHook postInstall
  '';

  passthru = { inherit tool creanga dorian; };

  meta = {
    description = "Word n-gram models (ro, en) and key click for maliit-keyboard on the Fairphone 6";
    license = with lib.licenses; [ publicDomain cc0 ];
  };
}
