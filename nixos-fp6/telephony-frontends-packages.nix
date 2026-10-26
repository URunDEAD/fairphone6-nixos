{ pkgs }:
let
  lomiri-online-accounts = pkgs.libsForQt5.callPackage ./online-accounts-bindings-package.nix { };
  sqlite-icu-extension = pkgs.callPackage ./sqlite-icu-extension-package.nix { };
  qtcontacts-sqlite = pkgs.libsForQt5.callPackage ./qtcontacts-sqlite-package.nix {
    inherit sqlite-icu-extension;
  };
  addressbook = pkgs.lomiri.callPackage ./addressbook-frontend-package.nix {
    inherit lomiri-online-accounts qtcontacts-sqlite;
  };
  frontend = kind: pkgs.lomiri.callPackage ./telephony-frontend-package.nix {
    inherit kind addressbook lomiri-online-accounts qtcontacts-sqlite;
  };
in { inherit addressbook lomiri-online-accounts qtcontacts-sqlite sqlite-icu-extension;
  dialer = frontend "dialer";
  messaging = frontend "messaging";
}
