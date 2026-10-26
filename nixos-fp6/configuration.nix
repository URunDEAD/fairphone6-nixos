{ config, lib, pkgs, ... }:
{
  system.stateVersion = "26.05";
  networking.hostName = "fp6";
  networking.networkmanager.enable = true;
  networking.useDHCP = false;
  networking.interfaces.usb0.ipv4.addresses = [{ address = "172.16.42.1"; prefixLength = 24; }];
  networking.firewall.allowedTCPPorts = [ 22 ];
  # The reused kernel has nftables but omits xt_pkttype and nft_fib_inet.
  networking.nftables.enable = true;
  networking.firewall.checkReversePath = false;
  networking.firewall.interfaces.usb0.allowedUDPPorts = [ 67 ];
  networking.networkmanager.unmanaged = [ "interface-name:usb0" ];
  networking.nameservers = [ "1.1.1.1" "9.9.9.9" ];
  services.resolved = { enable = true; settings.Resolve.DNSSEC = "false"; };
  services.openssh = {
    enable = true;
    settings = { PasswordAuthentication = true; PermitRootLogin = "no"; };
  };
  services.dnsmasq = {
    enable = true;
    settings = {
      port = 0;
      interface = "usb0";
      bind-dynamic = true;
      dhcp-range = [ "172.16.42.2,172.16.42.2,255.255.255.0,12h" ];
      dhcp-option = [ "3" "6" ];
    };
  };
  systemd.services.dnsmasq.preStart = "${pkgs.procps}/bin/pkill -x udhcpd || true";
  users.users.user = {
    isNormalUser = true;
    uid = 10000;
    extraGroups = [ "wheel" "networkmanager" "video" "audio" "input" ];
    hashedPasswordFile = "/etc/nixos-secrets/user-password";
  };
  security.sudo.wheelNeedsPassword = true;
  services.desktopManager.lomiri.enable = true;
  services.displayManager.defaultSession = "lomiri";
  hardware.graphics.enable = true;
  hardware.sensor.iio.enable = true;
  environment.sessionVariables.GRID_UNIT_PX = "24";
  environment.sessionVariables.DEVICEINFO_DEVICE_PATH = "/etc/deviceinfo/devices";
  environment.sessionVariables.QT_PLUGIN_PATH = "${pkgs.maliit-framework}/${pkgs.libsForQt5.qtbase.qtPluginPrefix}";
  environment.sessionVariables.MIR_SERVER_ADD_WAYLAND_EXTENSIONS = "zwp_input_panel_v1:zwp_input_method_v1";
  # LightDM sanitizes the greeter environment, so set this in its wrapper.
  services.xserver.displayManager.lightdm.extraSeatDefaults = lib.mkAfter ''
    greeter-wrapper = ${pkgs.writeShellScript "fp6-lomiri-greeter" ''
      export DEVICEINFO_DEVICE_PATH=/etc/deviceinfo/devices
      export MIR_SERVER_ADD_WAYLAND_EXTENSIONS=zwp_input_panel_v1:zwp_input_method_v1
      export QT_PLUGIN_PATH=${pkgs.maliit-framework}/${pkgs.libsForQt5.qtbase.qtPluginPrefix}:''${QT_PLUGIN_PATH:-}
      ${config.hardware.fairphone6.lomiriGreeterWrapperPre or ""}
      exec ${lib.getExe' pkgs.lomiri.lomiri "lomiri-greeter-wrapper"} "$@"
    ''}
  '';
  systemd.user.services.lomiri-keyboard = {
    description = "On-screen keyboard for Lomiri";
    wantedBy = [ "lomiri.service" ];
    after = [ "lomiri.service" ];
    partOf = [ "lomiri.service" "graphical-session.target" ];
    serviceConfig = {
      ExecStart = "${pkgs.maliit-keyboard}/bin/maliit-keyboard";
      Restart = "on-failure";
      RestartSec = 3;
      # The keyboard's OpenGL/llvmpipe renderer crashes on its first frame.
      Environment = [
        "QT_QPA_PLATFORM=wayland"
        "MALIIT_FORCE_DBUS_CONNECTION=1"
        "QT_QUICK_BACKEND=software"
        "QT_QUICK_CONTROLS_STYLE=Default"
      ];
    };
  };
  environment.etc."deviceinfo/devices/fairphone-fp6.yaml".text = ''
    fairphone-fp6:
      Names:
        - The Fairphone (Gen. 6)
        - Fairphone (Gen. 6)
      PrettyName: Fairphone (Gen. 6)
      DeviceType: phone
      GridUnit: 24
      WebkitDpr: 3
      PrimaryOrientation: Portrait
      SupportedOrientations:
        - Portrait
        - Landscape
        - InvertedLandscape
  '';
  documentation.nixos.enable = false;
  boot.kernelParams = [ "console=tty0" "loglevel=7" "ignore_loglevel" "printk.devkmsg=on" ];
  # Preserve the actual stage-1 exception in ramoops across a failed RAM boot.
  mobile.boot.stage-1.bootlog = { enable = false; kmsg = true; };
  boot.kernelModules = [ "panel_novatek_nt37705" "ath11k_ahb" "qcom_q6v5_pas" "inv_icm42600_spi" ];
  boot.growPartition = lib.mkForce false;
  fileSystems."/" = {
    device = "/dev/disk/by-label/NIXOS_SYSTEM";
    fsType = "ext4";
    autoResize = lib.mkForce false;
  };
  mobile.boot.boot-control.enable = false;
  # USB ADB is also part of the requested hardware acceptance tests.
  mobile.adbd.enable = true;
  nixpkgs.overlays = lib.mkAfter [(final: prev: {
    # Standalone Maliit must publish its visible area to input-context clients.
    maliit-framework = prev.maliit-framework.overrideAttrs (old: {
      patches = (old.patches or []) ++ [ ./maliit-standalone-input-area.patch ];
      doCheck = final.stdenv.buildPlatform.canExecute final.stdenv.hostPlatform;
      checkPhase = ''
        runHook preCheck
        export QT_PLUGIN_PATH=${lib.getBin final.libsForQt5.qtbase}/${final.libsForQt5.qtbase.qtPluginPrefix}
        ctest --output-on-failure -R '^ut_standaloneinputmethodarea$'
        runHook postCheck
      '';
    });
    # Backport Mir PR #3000 while retaining QtMir's supported graphics ABI.
    mir_2_15 = prev.mir_2_15.overrideAttrs (old: {
      patches = (old.patches or []) ++ [ ./mir-input-method-v1.patch ];
      doCheck = false;
    });
    lomiri = prev.lomiri.overrideScope (lfinal: lprev: {
      # Wayland clients use logical grid units; the compositor retains GridUnit 24.
      lomiri-session = lprev.lomiri-session.overrideAttrs (old: {
        patches = (old.patches or []) ++ [ ./lomiri-session-wayland-grid.patch ];
      });
      # Validate this hardware port on-device; omit the desktop test harness.
      lomiri = lprev.lomiri.overrideAttrs (old: {
        doCheck = false;
        patches = (old.patches or []) ++ [
          ./lomiri-input-method-scale.patch
          ./lomiri-wayland-surface-scale.patch
          ./lomiri-relocatable-installed-paths.patch
        ];
        postInstall = (old.postInstall or "") + lib.optionalString
          (final.stdenv.buildPlatform.canExecute final.stdenv.hostPlatform) ''
            $CXX -std=c++17 -fPIC -Iinclude \
              ../tests/installedpaths_test.cpp \
              $(pkg-config --cflags --libs Qt5Core Qt5Gui Qt5Qml) \
              -o $out/bin/.fp6-installed-paths-check
            ${final.buildPackages.python3.interpreter} ../tests/installedpaths_relocation.py \
              $out/bin/.fp6-installed-paths-check $out
            rm $out/bin/.fp6-installed-paths-check
          '';
        postFixup = lib.replaceStrings
          [ "wrapQtApp $out/bin/lomiri-mock-indicator-service" ]
          [ "" ] old.postFixup;
      });
      qtmir = lprev.qtmir.overrideAttrs (old: {
        patches = (old.patches or []) ++ [
          ./qtmir-scaled-buffer.patch
          ./qtmir-initial-focus.patch
          ./qtmir-initial-input-region.patch
          ./qtmir-initial-child-placement.patch
          ./qtmir-input-method-lifecycle.patch
        ];
        cmakeFlags = (old.cmakeFlags or []) ++ [
          (lib.cmakeBool "QTMIR_INPUTMETHOD_LIFECYCLE_TEST"
            (final.stdenv.buildPlatform.canExecute final.stdenv.hostPlatform))
        ];
        # Mir2's legacy test harness is unavailable; exercise the shared placement
        # helper natively without starting a compositor or touching hardware.
        postBuild = (old.postBuild or "") + lib.optionalString
          (final.stdenv.buildPlatform.canExecute final.stdenv.hostPlatform) ''
            $CXX -std=c++14 -fPIC -I../src/common \
              ../tests/initialchildplacement_test.cpp \
              $(pkg-config --cflags --libs Qt5Core) -o fp6-initial-child-placement-test
            ./fp6-initial-child-placement-test
            ./src/modules/QtMir/Application/qtmir-inputmethod-lifecycle-test
          '';
        # QtMir checks /proc/PID/exe before accepting an input-method surface.
        # This is the actual ELF behind the two Nix wrappers, verified on-device.
        postPatch = (old.postPatch or "") + ''
          substituteInPlace src/platforms/mirserver/wrappedwindowmanagementpolicy.cpp \
            --replace-fail 'maliitBinaryPath != "/usr/bin/maliit6-server"' \
            'maliitBinaryPath != "/usr/bin/maliit6-server" && maliitBinaryPath != "${final.maliit-keyboard}/bin/..maliit-keyboard-wrapped-wrapped"'
        '';
      });
    });
    # v1.3 knows the study/tunning partitions requested by the FP6 modem.
    rmtfs = prev.rmtfs.overrideAttrs (old: {
      version = "1.3";
      src = lib.cleanSource ./upstream/rmtfs;
    });
    gadget-tool = prev.gadget-tool.overrideAttrs (old: {
      cmakeFlags = (old.cmakeFlags or []) ++ [ "-DCMAKE_POLICY_VERSION_MINIMUM=3.5" ];
    });
  })];
  systemd.services.qbootctl = {
    description = "Mark the existing Qualcomm boot slot successful";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = "${pkgs.qbootctl}/bin/qbootctl -m";
    };
  };
  systemd.services.rmtfs = {
    description = "Qualcomm remote filesystem";
    wantedBy = [ "multi-user.target" ];
    before = [ "NetworkManager.service" ];
    unitConfig.ConditionPathExists = "/dev/qcom_rmtfs_mem1";
    serviceConfig = {
      ExecStart = "${pkgs.rmtfs}/bin/rmtfs -r -P -s";
      Restart = "always";
      RestartSec = 1;
    };
  };
  systemd.services.tqftpserv = {
    description = "Qualcomm firmware file service";
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      ExecStart = "${pkgs.tqftpserv}/bin/tqftpserv";
      StateDirectory = "tqftpserv";
      Restart = "always";
    };
  };
  environment.pathsToLink = [ "/share/uncompressed-firmware" ];
  environment.systemPackages = (with pkgs; [ vim curl git usbutils pciutils iw kdePackages.breeze-icons ]) ++ [ (pkgs.runCommand "fp6-userspace-firmware" {} ''
    mkdir -p $out/share
    ln -s ${config.mobile.device.firmware}/lib/firmware $out/share/uncompressed-firmware
  '') ];
  time.timeZone = "Europe/Bucharest";
  nix.settings.experimental-features = [ "nix-command" "flakes" ];
  # Allow persistent local edits without rebuilding the boot partition.
  environment.etc."nixos/fp6-build-source".text = "Custom FP6 Mobile NixOS port; see /run/nura/home/user/fp6-install/nixos-fp6\n";
}
