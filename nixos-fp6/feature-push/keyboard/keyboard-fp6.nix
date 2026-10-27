# Keyboard quality and Romanian support for maliit-keyboard (Lomiri OSK).
# Notes and evidence: results/feature-push-20261024/keyboard/README.md
#
# What was wrong on the phone (all found from the build outputs, no phone):
#  * Spell checking never ran: maliit-keyboard looks for hunspell files in
#    HUNSPELL_DICT_PATH, which upstream defaults to /usr/share/hunspell. That
#    does not exist on NixOS, so every language logged "No dictionary found"
#    and corrections/auto-correct were dead.
#  * There was no word prediction at all: upstream 2.3.1 removed presage
#    (maliit/keyboard #146, presage is unmaintained and not in nixpkgs), so the
#    word ribbon only ever showed the typed word.
#  * "Keyboard sound" played nothing: the GSettings default
#    sounds/key_tick2_quiet.wav is not shipped by maliit-keyboard.
#  * Keyboard vibration relied on the QtFeedback hfd plugin being found via an
#    inherited QT_PLUGIN_PATH; maliit's own wrapper did not include it.
#
# Fix:
#  * hunspell en_US + ro_RO (comma-below ș/ț) baked in as HUNSPELL_DICT_PATH.
#  * maliit-keyboard-ngram-prediction.patch: a small trigram/bigram/unigram
#    predictor (ngrampredictor.cpp) behind the existing SpellPredictWorker,
#    fed by models built here (package.nix, fp6-kbd-ngram in Rust) from
#    public-domain/CC0 corpora; diacritic-insensitive completion ("si" ->
#    "și", "fata" -> "față"); predictions filtered through hunspell; words
#    the user types are learned into ~/.local/share/maliit-keyboard/
#    <lang>_ngram_user.tsv (persists across reboots, next to the existing
#    <lang>_userDictionary.dic of "add to dictionary").
#  * maliit-keyboard-ro-layout-space-swipe-mic-slot.patch: Romanian layout
#    with ț first on t (long-press gives ț; ă on a, ș on s, î on i, â second
#    on a), „ ” on the comma key; quick horizontal swipe on space switches
#    language (globe key still taps to the next, long-press for the menu);
#    a zero-width MicKey slot for the dictation bridge (not wired).
#  * enabled-languages default ['en', 'ro'] as a dconf system-db value (the
#    user's own choice in Settings > Language & Text > Keyboard layouts wins).
#  * Generated key click WAV at the path the GSettings default names, and the
#    hfd QtFeedback plugin on maliit's QT_PLUGIN_PATH (vibration strength is
#    still misc/haptics-fp6.nix).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.keyboard;
in {
  options.hardware.fairphone6.keyboard = {
    enable = lib.mkEnableOption "maliit-keyboard spell checking, word prediction and Romanian support";
    hunspellDictionaries = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ pkgs.hunspellDicts.en_US pkgs.hunspellDicts.ro_RO ];
      defaultText = lib.literalExpression "[ pkgs.hunspellDicts.en_US pkgs.hunspellDicts.ro_RO ]";
      description = ''
        Hunspell dictionaries maliit-keyboard uses (matched by layout id:
        "ro" finds ro_RO). Keep one dictionary per language: maliit takes the
        first match in alphabetical order.
      '';
    };
    defaultEnabledLanguages = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "en" "ro" ];
      description = ''
        Default org.maliit.keyboard.maliit enabled-languages (dconf system
        database; a value the user sets in Settings wins).
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = [ (import ./overlay.nix { inherit (cfg) hunspellDictionaries; }) ];

    programs.dconf.enable = true;
    programs.dconf.profiles.user.databases = [{
      settings."org/maliit/keyboard/maliit" = {
        enabled-languages = cfg.defaultEnabledLanguages;
      };
    }];

    system.build.fp6-keyboard-models = pkgs.maliit-keyboard.passthru.fp6Keyboard.models;
    system.build.fp6-keyboard-maliit = pkgs.maliit-keyboard;

    assertions = [
      {
        assertion = pkgs.maliit-keyboard.passthru ? fp6Keyboard;
        message = "keyboard-fp6: the maliit-keyboard overlay was not applied.";
      }
      {
        assertion = config.systemd.user.services.lomiri-keyboard.enable;
        message = "keyboard-fp6 configures maliit-keyboard as run by lomiri-keyboard.service.";
      }
    ];
  };
}
