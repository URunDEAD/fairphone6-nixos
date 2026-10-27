# Mir upgrade prototype (2026-10-27): Lomiri's qtmir and Lomiri on the
# current nixpkgs Mir (2.28.0) instead of the pinned mir_2_15.
# Notes and the RAM-boot checklist: results/feature-push-20261024/mir-upgrade/README.md.
#
# Default OFF: with enable = false this module adds nothing (the default
# kernel6 toplevel stays byte-identical). Wrapper target with it on:
# trial-lomiri-full-kernel6-mir228-fp6.nix. Build/eval only.
#
#  * Only qtmir and lomiri link Mir (closure check: the only referrers of
#    mir-2.15.0.drv). Maliit, lomiri-system-settings and the apps are plain
#    Wayland clients and are not rebuilt.
#  * mir_2_15 is replaced inside the lomiri scope only (callPackage of qtmir
#    and lomiri picks it up, every FP6 overrideAttrs layer stays). Mir itself
#    is nixpkgs' mir unchanged (binary cache). mir-input-method-v1.patch is
#    not needed: zwp_input_method_v1 / zwp_input_panel_v1 are upstream since
#    Mir 2.16 (MIR_SERVER_ADD_WAYLAND_EXTENSIONS still adds them).
#  * qtmir: nixpkgs' pin (debian-upstream 57d9e976) carries the Mir
#    platform-API port up to Mir 2.20 only; qtmir-mir2-28-upstream.patch adds
#    five later upstream fixes (EDID -> DisplayInfo for 2.22, kwin
#    server-decoration now in Mir 2.28, DisplayListenerWrapper, null
#    GLRenderingProvider guard, demo EDID). Built with -DWerror=OFF and libgbm.
#  * lomiri: lomiri-mir2-28.patch (no EDID descriptors on Mir >= 2.22), C++20.
#  * qtmir-mir2-28-fp6.patch: the privacy-lock restriction also covers Mir
#    2.28's new privileged globals; Mir's logind-driven session lock is logged.
#  * With privacyLock.clipboard (default on with this option):
#    qtmir-data-control-clipboard-helper.patch offers ext_data_control_manager_v1
#    to fp6-privacy-clipboard only, and Settings lists the Clipboard page.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.mirUpgrade;
in {
  options.hardware.fairphone6.mirUpgrade = {
    enable = lib.mkEnableOption ''
      Lomiri (qtmir + lomiri) on nixpkgs' current Mir (2.28) instead of
      mir_2_15: ext-data-control (clipboard privacy helper only),
      wp_viewporter / fractional-scale, newer text-input/IME code. Prototype:
      built, not yet booted'';
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = (config.hardware.fairphone6.privacyLock.enable or false)
        && config.hardware.fairphone6.privacyLock.restrictWaylandExtensions;
      message = "mirUpgrade: the qtmir patches build on privacy-lock's qtmir-privileged-wayland-extensions.patch (privacyLock.enable + restrictWaylandExtensions).";
    }];

    # After every other qtmir layer (privacy-lock is 2800, notifications 2900).
    nixpkgs.overlays = lib.mkOrder 3500 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        mir_2_15 = final.mir;
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [
            ./qtmir-mir2-28-upstream.patch
            ./qtmir-mir2-28-fp6.patch
          ] ++ lib.optional config.hardware.fairphone6.privacyLock.clipboard
            ./qtmir-data-control-clipboard-helper.patch;
          # Mir 2.28's <mir/graphics/platform.h> includes gbm.h.
          buildInputs = (old.buildInputs or [ ]) ++ [ final.libgbm ];
          # miral id_for_window / apply_lifecycle_state_to are deprecated in
          # 2.28 (still implemented); qtmir builds with -Werror by default.
          cmakeFlags = (old.cmakeFlags or [ ]) ++ [ (lib.cmakeBool "Werror" false) ];
          passthru = (old.passthru or { }) // { fp6Mir = final.mir.version; };
        });
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-mir2-28.patch ];
          # Mir 2.28 headers need C++20 (concepts, defaulted comparisons).
          postPatch = (old.postPatch or "") + ''
            substituteInPlace CMakeLists.txt \
              --replace-fail 'set (CMAKE_CXX_STANDARD 17)' 'set (CMAKE_CXX_STANDARD 20)'
          '';
          buildInputs = (old.buildInputs or [ ]) ++ [ final.libgbm ];
        });
        lomiri-system-settings =
          if !config.hardware.fairphone6.privacyLock.clipboard then lprev.lomiri-system-settings
          else lprev.lomiri-system-settings.overrideAttrs (old: {
            # After privacy-lock's lss-security-privacy.patch (same postInstall).
            postInstall = (old.postInstall or "") + ''
              patch -d "$out/share/lomiri-system-settings/qml-plugins/security-privacy" -p1 --forward --batch \
                < ${./lss-security-privacy-clipboard.patch}
              grep -q fp6Clipboard "$out/share/lomiri-system-settings/qml-plugins/security-privacy/PageComponent.qml"
            '';
          });
      });
    }) ];

    system.build.fp6-mir-upgrade = {
      inherit (pkgs) mir;
      inherit (pkgs.lomiri) qtmir lomiri lomiri-system-settings;
    };
  };
}
