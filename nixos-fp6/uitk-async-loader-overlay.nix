# Rebuild toolkit consumers against the loader ownership repair.
_final: prev: {
  lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
    lomiri-ui-toolkit = lprev.lomiri-ui-toolkit.overrideAttrs (old: {
      patches = (old.patches or []) ++ [ ./uitk-async-loader-lifetime.patch ];
    });
  });
}
