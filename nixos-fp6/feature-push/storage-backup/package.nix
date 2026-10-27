# Storage manager + Backup (feature push 2026-10-24).
#  core:      Rust crate (rust/): fp6-storaged (root helper) and
#             fp6-storage-agent (session service, scheduled backup run),
#             plus the user units for the daily backup timer.
#  lssPlugin: System Settings > Storage and > Backup (UITK QML) and the
#             Fp6.StorageBackup QML module (one Qt D-Bus bridge class and
#             the shared AgentModel.qml).
{ lib, stdenv, rustPlatform, pkg-config, sqlite, makeWrapper, cmake, libsForQt5, dconf, systemd }:
let
  core = rustPlatform.buildRustPackage {
    pname = "fp6-storage-backup";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    nativeBuildInputs = [ pkg-config makeWrapper ];
    buildInputs = [ sqlite ];
    # cargo test (synthetic data only): boot image cmdline pins and the
    # generation keep plan, store closure accounting, du/cache clearing on a
    # temp tree, sqlite snapshot of an open WAL db and restore into it,
    # archive write/verify/tamper/truncate, key wrap/passphrase change,
    # full backup -> restore on a "new phone", restore path confinement.
    doCheck = true;
    postInstall = ''
      wrapProgram $out/bin/fp6-storage-agent --prefix PATH : ${lib.makeBinPath [ dconf systemd ]}
      mkdir -p $out/lib/systemd/user
      cat > $out/lib/systemd/user/fp6-backup.timer <<UNIT
      [Unit]
      Description=Daily backup (Settings > Backup)

      [Timer]
      OnCalendar=*-*-* 03:30
      RandomizedDelaySec=30min
      Persistent=true

      [Install]
      WantedBy=timers.target
      UNIT
      cat > $out/lib/systemd/user/fp6-backup.service <<UNIT
      [Unit]
      Description=Scheduled backup (Settings > Backup)

      [Service]
      Type=oneshot
      ExecStart=$out/bin/fp6-storage-agent scheduled
      Nice=10
      IOSchedulingClass=idle
      UNIT
      sed -i 's/^      //' $out/lib/systemd/user/*
    '';
    meta = {
      description = "Storage manager and encrypted backup/restore for Lomiri on the Fairphone 6";
      license = lib.licenses.mit;
      mainProgram = "fp6-storage-agent";
      platforms = lib.platforms.linux;
    };
  };

  lssPlugin = stdenv.mkDerivation {
    pname = "lomiri-system-settings-storage-backup";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: type:
        let rel = lib.removePrefix (toString ./. + "/") (toString path); in
        lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "qml-plugin" "settings-page" ];
    };
    sourceRoot = "source/qml-plugin";
    nativeBuildInputs = [ cmake ];
    buildInputs = [ libsForQt5.qtbase libsForQt5.qtdeclarative ];
    dontWrapQtApps = true;
    postInstall = ''
      for p in storage backup; do
        install -Dm644 $NIX_BUILD_TOP/source/settings-page/$p/$p.settings $out/share/lomiri-system-settings/$p.settings
        install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/$p $NIX_BUILD_TOP/source/settings-page/$p/*.qml
      done
      # The wrapper lndir's these from every plugin.
      mkdir -p $out/share/locale
      test -e $out/lib/lomiri-system-settings/private/Fp6/StorageBackup/libfp6storagebackupplugin.so
    '';
    meta = {
      description = "Storage and Backup pages for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit core lssPlugin; }
