# AppArmor 5's parser install omits the already patched init shell helper.
# NixOS aa-remove-unknown and aa-teardown both source it from the parser output.
_final: prev: {
  apparmor-parser = prev.apparmor-parser.overrideAttrs (old: {
    postInstall = (old.postInstall or "") + ''
      install -Dm644 ../init/rc.apparmor.functions \
        "$out/lib/apparmor/rc.apparmor.functions"
    '';
  });
}
