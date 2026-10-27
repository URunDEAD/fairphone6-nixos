# Compile-only check of the kernel7 candidate 0003 (no stale CHG reads,
# gesture_wakeup errors) on top of sensors/0003 and kernel7 0001, as an
# external module against the kernel6 (7.2.0-fp6-nixos6) headers. Reuses
# compile-check.nix; see there. The .ko is evidence only.
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file kernel-build/feature-push/kernel7-candidates/eswin-lp-compile-check.nix --no-link --print-out-paths
args@{ ... }:
(import ./compile-check.nix args).overrideAttrs (old: {
  pname = "fp6-kernel7-candidate-eswin-lp-compile-check";
  patches = old.patches ++ [
    ./0003-input-eswin-eph861x-no-stale-chg-reads-report-gesture-errors.patch
  ];
  checkPhase = old.checkPhase + ''
    grep -q 'eph_read_chg(ephdata)' drivers/input/touchscreen/eswin_touch/eswin_eph861x.c
    # W=1: no warnings at all in eswin_eph861x.c (the file this patch touches).
    ! grep -n 'eswin_eph861x\.c:[0-9]*:[0-9]*: warning' build-eswin.log
  '';
})
