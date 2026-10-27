# The full-features Lomiri GPU target (trial-lomiri-full-fp6.nix) on kernel6:
# kernel-build/feature-push/kernel6 = exact kernel5 recipe plus the UFS
# system-suspend fix for milos (kernel-build/feature-push/suspend/0001-...:
# "qcom,milos-ufshc" gets no_phy_retention -> default spm_lvl 5, as upstream
# does for SM8550/SM8650) and CONFIG_PM_DEBUG (/sys/power/pm_test for the
# staged, supervised first suspend test). modDirVersion 7.2.0-fp6-nixos6.
# Driver/config-only: the separately built, preserved DTB is unchanged.
# Suspend stays GUARDED (AllowSuspend=no); udev also writes spm_lvl=5.
# See results/feature-push-20261024/suspend/README.md.
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  kernelPin = {
    kernelBuild = ../../kernel-build/feature-push/kernel6;
    expectedKernelDrvPath = "/nix/store/g53ral2ypy7l6p5v943yngh12bi26qms-linux-fp6-nixos-7.2.0.drv";
    kernelRelease = "7.2.0-fp6-nixos6";
    # sha256 of the kernel6 builds (checked at runtime by the selfie boot-load unit):
    # fp6-s5kkd1.ko from s02slx2w...-fp6-s5kkd1-driver-diagnostic-0.3-kernel4-stream-reads,
    # updates/qcom-camss.ko from zc8hrrdl...-fp6-camss-kernel4-rdi16-1 (both vermagic nixos6).
    # Front camera Bayer fix (camera phase 0): the fp6-s5kkd1 module is the
    # same 0.3 source rebuilt with MEDIA_BUS_FMT_SGBRG10_1X10
    # (camera/selfie-gbrg-fp6.nix); the plain 0.3 build was
    # 6aa174a532756863a22fc83a8147f60bb6abe806ab53974606428c30579e2ef5.
    selfieModuleSha256 = "21cfae84945c73fd1f67f73db289dd6f87c2a4892f12e502eac0ec0829b34fda";
    expectedCamssSha256 = "7629d4dd284c12ed84f2a6ee63e4ae48a8444bb8b271986e25e33684d53ce8e7";
  };
  base = import ./trial-lomiri-full-fp6.nix { inherit nixpkgs kernelPin; };
  evaluated = base.eval.extendModules {
    modules = [ ./camera/selfie-gbrg-fp6.nix
      # Lomiri file picker for GTK4 (Fragments), Firefox and portal-using Qt
      # apps: xdg-desktop-portal FileChooser backend.
      # results/feature-push-20261024/file-picker/README.md
      ({ ... }: { imports = [ ./file-picker/file-picker-fp6.nix ]; hardware.fairphone6.filePickerPortal.enable = true; })
      # Quick Share (Android nearby sharing over Wi-Fi), built by a Codex worker:
      # nixos-fp6/feature-push/quick-share/README.md
      ({ ... }: { imports = [ ./quick-share/module.nix ]; hardware.fairphone6.quickShare.enable = true; })
      # Share via link/QR (any device on the same Wi-Fi or hotspot), built by a Codex worker:
      # nixos-fp6/feature-push/webshare/README.md
      ({ ... }: { imports = [ ./webshare/module.nix ]; hardware.fairphone6.webshare.enable = true; })
      # Text recognition (offline OCR, English/Romanian), built by a Codex worker:
      # nixos-fp6/feature-push/ocr/README.md
      ({ ... }: { imports = [ ./ocr/module.nix ]; hardware.fairphone6.ocr.enable = true; })
      # Bug report (privacy-redacted diagnostics bundle), built by a Codex worker:
      # nixos-fp6/feature-push/bugreport/README.md
      ({ ... }: { imports = [ ./bugreport/module.nix ]; hardware.fairphone6.bugreport.enable = true; })
      # KDE Connect (desktop/Android integration), built by a Codex worker:
      # nixos-fp6/feature-push/kdeconnect/README.md
      ({ ... }: { imports = [ ./kdeconnect/module.nix ]; hardware.fairphone6.kdeconnect.enable = true; hardware.fairphone6.kdeconnect.user = "user"; })
      # Offline voice dictation service (Whisper; model downloaded on demand), built by a Codex worker:
      # nixos-fp6/feature-push/dictation/README.md
      ({ ... }: { imports = [ ./dictation/module.nix ]; hardware.fairphone6.dictation.enable = true; })
      # USB preferences: System Settings > USB (USB network / file transfer /
      # USB drive / ADB toggles, power role), "Charging via USB" notification.
      # Developer build: usb0 (172.16.42.1) is always kept for SSH over USB.
      # results/feature-push-20261024/usb-modes/README.md
      ({ ... }: { imports = [ ./usb-modes/usb-modes-fp6.nix ];
                  hardware.fairphone6.usbModes = { enable = true; defaultFunctions = [ "ncm" ]; keepFunctions = [ "ncm" ]; }; })
      # White-box text test (2026-10-27): GPU preemption off via msm.enable_preemption=0.
      # results/feature-push-20261024 white-boxes notes; the Mesa CCHE backport stays off.
      ({ ... }: { imports = [ ./white-boxes/white-boxes-fp6.nix ]; hardware.fairphone6.whiteBoxes.gpuPreemptionOff = true; })
      # OTA updates (Settings > System update). Slot switching OFF: installs
      # rewrite the running boot partition with a backup, no automatic rollback.
      # results/feature-push-20261024/ota/README.md
      ({ ... }: { imports = [ ./ota/ota-fp6.nix ]; hardware.fairphone6.ota.enable = true; hardware.fairphone6.ota.allowSlotSwitch = false; })
      # Call recording: fp6-callrec user service + dialer Record button,
      # settings and Recordings list. Recording stays OFF until the user
      # picks a mode in Phone > Settings > Call recording. CS in-call record
      # (csIncallRecord) stays off until cs-call-audio works.
      # results/feature-push-20261024/call-recording/README.md
      ({ ... }: { imports = [ ./call-recording/call-recording-fp6.nix ]; hardware.fairphone6.callRecording.enable = true; })
      # App switcher: close all apps (drag right past the first app, or Clear all);
      # Settings > Gesture Shortcuts > App switcher.
      # results/feature-push-20261024/spread-close-all/README.md
      ({ ... }: { imports = [ ./spread-close-all/spread-close-all-fp6.nix ]; hardware.fairphone6.spreadCloseAll.enable = true; })
      # State across reboots: VPNs reconnect at startup (Settings > VPN >
      # Reconnect at startup: restore last state / always on / off).
      # results/feature-push-20261024/persistence/README.md
      ({ ... }: { imports = [ ./persistence/persistence-fp6.nix ]; hardware.fairphone6.persistence.enable = true; })
      # Settings > Accessibility: magnification (triple-tap / hold / volume keys,
      # pinch + two-finger pan), colour inversion and correction, mono audio,
      # balance; quick-settings tiles. results/feature-push-20261024/accessibility/README.md
      ({ ... }: { imports = [ ./accessibility/accessibility-fp6.nix ]; hardware.fairphone6.accessibility.enable = true; })
      # Edge lighting: screen-edge glow for notifications while the display
      # is off (Settings > Edge Lighting, off by default); always-on display
      # research. results/feature-push-20261024/edge-flash/README.md
      ({ ... }: { imports = [ ./edge-flash/edge-flash-fp6.nix ]; hardware.fairphone6.edgeFlash.enable = true; })
      ({ config, lib, ... }: let
      kernel = config.mobile.boot.stage-1.kernel.package;
    in {
      # stk3310 (STK36C61 ALS/proximity, CONFIG_STK3310=m) autoloads through
      # its OF modalias (of:...Csensortek,stk36c61Csensortek,stk3310; bound by
      # udev on kernel4). Listing it as well makes the load deterministic even
      # if the i2c-gpio bus appears after the coldplug.
      boot.kernelModules = [ "stk3310" ];
      # Top mic on: UCM reads ADC2 from SWR_MIC0 (results/feature-push-20261024/audio/mic2-README.md).
      hardware.fairphone6.audioFeature.topMic = true;
      # kernel6 (like kernel5) carries the ESWIN lp_mode patch.
      services.fp6LomiriSensors.doubleTapToWake = true;
      system.nixos.tags = [ "kernel6" ];
      system.build.fp6-kernel6 = kernel;
      services.fp6SuspendPolicy.ufsLinkOffAtBoot = true;
      assertions = [{
        assertion = kernel.drvPath == kernelPin.expectedKernelDrvPath
          && kernel.modDirVersion == kernelPin.kernelRelease
          && config.boot.kernelPackages.kernel.modDirVersion == kernelPin.kernelRelease
          && config.system.build.fp6-camss-kernel4.passthru.kernelRelease == kernelPin.kernelRelease
          && config.system.build.fp6-selfie-kernel4-driver.passthru.kernelRelease == kernelPin.kernelRelease;
        message = "The kernel6 full target must use kernel6 for the kernel and every out-of-tree module.";
      }];
    }) ];
  };
in base // {
  eval = evaluated;
  inherit kernelPin;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
