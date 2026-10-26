# Separate opt-in successor to the physically tested03xd trial. Existing explicit
# load/fallback targets remain unchanged. Enables permission to stream, not capture.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/selfie-dphy
, baselineKernelBuild ? ../kernel-build/qcomtee-cma
# Kernel pin. The defaults are the exact DPHY kernel4; kernel5 targets
# (feature-push/trial-lomiri-full-kernel5-fp6.nix) pass their own values.
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
, selfieModuleSha256 ? "7860ee42ada71bac02eadae431f3accf55f117a9ae99ee152d4fc66688ac46e1"
, expectedCamssSha256 ? "6583a5c5912a343d6feb3a018c071583900ba902460abd2144d08c9b208f2ad0"
}:
# An isolated successor may pin a newly reviewed exact4 CAMSS replacement.
# The default preserves the previous artifact; no broad acceptance bypass.
assert builtins.match "[0-9a-f]{64}" expectedCamssSha256 != null;
let
  base = import ./trial-phosh-selfie-csid-kernel4-fp6.nix {
    inherit nixpkgs kernelBuild baselineKernelBuild expectedKernelDrvPath kernelRelease;
  };
  evaluated = base.eval.extendModules {
    modules = [ ({ config, pkgs, ... }: let
      module = "${config.system.build.fp6-selfie-kernel4-driver}/share/fp6-selfie-diagnostic/fp6-s5kkd1.ko";
      firmware = "${config.system.build.fp6-selfie-kernel4-firmware}/lib/firmware/fp6-s5kkd1-v1.bin";
      dtb = config.system.build.fp6-selfie-boot-dtb;
      load = pkgs.writeShellApplication {
        name = "fp6-selfie-kernel4-boot-load";
        runtimeInputs = [ pkgs.coreutils pkgs.kmod ];
        text = ''
          fail() { echo "FP6 selfie boot load refused: $*" >&2; exit 1; }
          [ "$(uname -r)" = '${kernelRelease}' ] || fail 'requires exact kernel4'
          printf '%s  %s\n' \
            '${selfieModuleSha256}' '${module}' \
            '42e47a8efaa51e1370599a827f5a45603e54220265f5763cfb79197703c20f68' '${firmware}' \
            '93bfe406603df92627f43d9740b201d6b5781da40773105c627ccdd6580e65cc' '${dtb}' \
            | sha256sum --check --status || fail 'module/firmware/DTB hash mismatch'
          [ "$(modinfo -F vermagic '${module}')" = '${kernelRelease} SMP preempt mod_unload aarch64' ] || fail 'module ABI mismatch'
          node=/sys/firmware/devicetree/base/soc@0/cci@ac16000/i2c-bus@1/camera@3d
          [ -r "$node/compatible" ] || fail 'reviewed front sensor DT node absent'
          [ "$(tr -d '\000' < "$node/compatible")" = 'samsung,s5kkd1' ] || fail 'wrong sensor DT compatible'
          tree=/run/booted-system/kernel-modules
          camss=$(modinfo -b "$tree" -k ${kernelRelease} -n qcom_camss) || fail 'CAMSS cannot resolve'
          case "$camss" in
            "$tree"/lib/modules/${kernelRelease}/updates/qcom-camss.ko) ;;
            *) fail 'CAMSS replacement is not selected' ;;
          esac
          printf '%s  %s\n' '${expectedCamssSha256}' "$camss" \
            | sha256sum --check --status || fail 'CAMSS hash mismatch'
          verify_loaded() {
            [ "$(cat /sys/module/fp6_s5kkd1/version)" = '0.3-stream-read-diagnostics' ] || fail 'different loaded sensor version'
            [ "$(cat /sys/module/fp6_s5kkd1/srcversion)" = "$(modinfo -F srcversion '${module}')" ] || fail 'different loaded sensor source'
            [ "$(cat /sys/module/fp6_s5kkd1/parameters/enable_stream)" = Y ] || fail 'loaded module has stream permission disabled; explicit recovery required'
          }
          if [ -d /sys/module/fp6_s5kkd1 ]; then
            verify_loaded
          else
            # Normal dependency loading only; never force, replace or unload.
            modprobe -d "$tree" -S ${kernelRelease} qcom_camss
            modprobe -d "$tree" -S ${kernelRelease} v4l2_cci
            insmod '${module}' enable_stream=1
            verify_loaded
          fi
          # insmod success alone does not prove probe succeeded. Require the
          # strict-ID driver's binding to this exact DT node within ten seconds.
          for ((attempt=0; attempt<50; attempt++)); do
            for device in /sys/bus/i2c/drivers/fp6-s5kkd1/*-003d; do
              if [ -L "$device/of_node" ] && [ "$(readlink -f "$device/of_node")" = "$node" ]; then
                echo 'FP6 selfie exact0.3 bound after strict ID probe; stream permission enabled; no capture started'
                exit 0
              fi
            done
            sleep 0.2
          done
          fail 'sensor did not bind; inspect kernel probe/deferred-probe logs (module left loaded)'
        '';
      };
    in {
      system.build.fp6-selfie-kernel4-boot-load = load;
      systemd.services.fp6-selfie-kernel4 = {
        description = "Load the physically tested exact kernel4 FP6 selfie sensor";
        wantedBy = [ "multi-user.target" ];
        after = [ "systemd-modules-load.service" "systemd-udev-trigger.service" ];
        before = [ "user@10000.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${load}/bin/fp6-selfie-kernel4-boot-load";
          TimeoutStartSec = 20;
          Restart = "no";
          PrivateTmp = true;
          ProtectHome = true;
          ProtectSystem = "strict";
        };
        # No ExecStop/rmmod: switching/stopping the service must not tear down
        # a sensor owned by an application. Runtime PM retains existing behavior.
      };
    }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
