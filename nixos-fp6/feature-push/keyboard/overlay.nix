# maliit-keyboard with spell checking, n-gram prediction and the Romanian
# layout changes; see keyboard-fp6.nix. Also used by test/default.nix.
{ hunspellDictionaries ? null }:
final: prev:
let
  lib = final.lib;
  models = final.callPackage ./package.nix { };
  dicts = final.symlinkJoin {
    name = "fp6-keyboard-hunspell-dicts";
    paths = if hunspellDictionaries != null then hunspellDictionaries
      else [ final.hunspellDicts.en_US final.hunspellDicts.ro_RO ];
  };
  canExecute = final.stdenv.buildPlatform.canExecute final.stdenv.hostPlatform;
  qtPluginPrefix = final.libsForQt5.qtbase.qtPluginPrefix;
in {
  maliit-keyboard = prev.maliit-keyboard.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [
      ./maliit-keyboard-ngram-prediction.patch
      ./maliit-keyboard-ro-layout-space-swipe-mic-slot.patch
    ];
    cmakeFlags = (old.cmakeFlags or [ ]) ++ [
      "-DHUNSPELL_DICT_PATH=${dicts}/share/hunspell"
    ];
    # Predictor unit test + C++/Rust parity on the real models (2981
    # queries). Same style as lomiri-polish's panel-size test.
    postBuild = (old.postBuild or "") + lib.optionalString canExecute ''
      $CXX -std=c++17 -fPIC -I../plugins/westernsupport \
        ../tests/fp6/ngrampredictor_test.cpp ../plugins/westernsupport/ngrampredictor.cpp \
        $(pkg-config --cflags --libs Qt5Core) -o fp6-ngrampredictor-test
      ./fp6-ngrampredictor-test \
        ${models}/share/fp6-keyboard/ngram_ro.tsv ${models}/share/fp6-keyboard/parity_ro.tsv \
        ${models}/share/fp6-keyboard/ngram_en.tsv ${models}/share/fp6-keyboard/parity_en.tsv
    '';
    postInstall = (old.postInstall or "") + ''
      for l in en ro; do
        test -d $out/lib/maliit/keyboard2/languages/$l
        install -m644 ${models}/share/fp6-keyboard/ngram_$l.tsv $out/lib/maliit/keyboard2/languages/$l/
      done
      install -Dm644 ${models}/share/fp6-keyboard/key_tick.wav \
        $out/share/maliit/keyboard2/sounds/key_tick2_quiet.wav
      grep -q "$out/share/maliit/keyboard2/sounds/key_tick2_quiet.wav" \
        $out/share/glib-2.0/schemas/org.maliit.keyboard.maliit.gschema.xml
      test -e $out/lib/maliit/keyboard2/qml/keys/MicKey.qml
    '';
    preFixup = (old.preFixup or "") + ''
      qtWrapperArgs+=(--prefix QT_PLUGIN_PATH : ${final.lomiri.hfd-service}/${qtPluginPrefix})
    '';
    passthru = (old.passthru or { }) // {
      fp6Keyboard = { inherit models dicts; };
    };
  });
}
