# Lomiri QML disk cache (2026-10-26). Imported by boot-speed-fp6.nix; active
# with hardware.fairphone6.bootSpeed.enable and .lomiriQmlCache (default on).
# Notes and measurements: results/feature-push-20261024/boot-speed/README.md
#
# Why Lomiri never wrote a QML cache: nixpkgs' Qt 5.15 qtdeclarative carries
# qtdeclarative-default-disable-qmlcache.patch, which makes
# QQmlTypeLoader::Blob::diskCacheEnabled() return forceDiskCache() only. The
# disk cache, and also prebuilt .qmlc files next to the .qml (qmlcachegen
# output), are used only with QML_FORCE_DISK_CACHE set. QML_DISABLE_DISK_CACHE
# is not set anywhere, and both cache dirs are writable. So ahead-of-time
# .qmlc files would need the same variable. Only units compiled into the
# binary (qtquickcompiler/qrc) bypass the gate, and Lomiri installs plain .qml.
#
# nixpkgs disables the cache because Qt checks the cache against the source
# mtime, and every store file has mtime 1. On NixOS that is safe only when the
# source URL is immutable. Lomiri (greeter and session) loads QML only from
# /nix/store paths: its own share/lomiri plus NIXPKGS_QT5_QML_IMPORT_PATH, with
# no QML2_IMPORT_PATH and no /run/current-system paths (checked on the phone,
# 2026-10-26). A changed module therefore has a new URL, and so a new cache
# file name. The prepare step below also wipes the cache when the
# qtdeclarative build changes, and prunes entries unused for 14 days
# (orphans from old Lomiri builds).
#
# Scope: only the Lomiri process. The greeter gets the variable through the
# configuration.nix greeter wrapper (hardware.fairphone6.lomiriGreeterWrapperPre),
# and the session through a lomiri.service drop-in. Apps start from the systemd
# user manager's environment, so they are not affected.
# Caches: /var/lib/lightdm/.cache/UBports/lomiri/qmlcache (greeter),
# ~/.cache/UBports/lomiri/qmlcache (session); ~4 MB each.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.bootSpeed;
  enabled = cfg.enable && cfg.lomiriQmlCache;
  qtdeclarative = pkgs.libsForQt5.qtdeclarative;
  stamp = "fp6-qmlcache-1 ${qtdeclarative}";
  prepare = pkgs.writeShellScript "fp6-lomiri-qmlcache-prepare" ''
    PATH=${lib.makeBinPath [ pkgs.coreutils pkgs.findutils ]}
    dir=''${XDG_CACHE_HOME:-$HOME/.cache}/UBports/lomiri/qmlcache
    stamp=${lib.escapeShellArg stamp}
    if [ -d "$dir" ]; then
      if [ "$(cat "$dir/.fp6-qmlcache-stamp" 2>/dev/null)" != "$stamp" ]; then
        rm -rf "$dir"
      else
        find "$dir" -maxdepth 1 -name '*.qmlc' -atime +14 -delete
      fi
    fi
    mkdir -p "$dir" || exit 0
    [ -f "$dir/.fp6-qmlcache-stamp" ] || printf '%s\n' "$stamp" > "$dir/.fp6-qmlcache-stamp"
    exit 0
  '';
  sessionDropin = pkgs.writeTextDir "lib/systemd/user/lomiri.service.d/fp6-qmlcache.conf" ''
    [Service]
    Environment=QML_FORCE_DISK_CACHE=1
    ExecStartPre=-${prepare}
  '';
in
{
  options.hardware.fairphone6 = {
    bootSpeed.lomiriQmlCache = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Let the Lomiri greeter and session use Qt's QML disk cache
        (QML_FORCE_DISK_CACHE=1, Lomiri process only). Effective only with
        hardware.fairphone6.bootSpeed.enable.
      '';
    };
    lomiriGreeterWrapperPre = lib.mkOption {
      type = lib.types.lines;
      default = "";
      internal = true;
      description = "Shell lines run by the LightDM greeter wrapper (configuration.nix) before Lomiri starts.";
    };
  };

  config = lib.mkIf enabled {
    hardware.fairphone6.lomiriGreeterWrapperPre = ''
      ${prepare} || true
      export QML_FORCE_DISK_CACHE=1
    '';
    systemd.packages = [ sessionDropin ];   # also provides user units (lib/systemd/user)
    system.build.fp6-lomiri-qmlcache-prepare = prepare;
  };
}
