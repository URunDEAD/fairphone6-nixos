{ config, lib, pkgs, ... }:
{
  options.mobile.hardware.socs.qualcomm-sm7635.enable = lib.mkEnableOption "SM7635";
  config = {
    mobile.device = {
      name = "fairphone-fp6";
      identity = { name = "Fairphone (Gen. 6)"; manufacturer = "Fairphone"; };
      supportLevel = "unsupported";
      firmware = pkgs.runCommand "fp6-firmware" {} ''
        mkdir -p $out/lib
        tar xf ${../assets/fp6-firmware.tar} -C $out/lib
        cp $out/lib/firmware/postmarketos/* $out/lib/firmware/
      '';
    };
    mobile.hardware = {
      soc = "qualcomm-sm7635";
      ram = 8192;
      screen = { width = 1116; height = 2484; };
    };
    mobile.system.system = "aarch64-linux";
    mobile.system.type = "android";
    mobile.system.android = {
      ab_partitions = true;
      boot_as_recovery = false;
      has_recovery_partition = false;
      device_name = "FP6";
      bootimg.flash = {
        offset_base = "0x00000000";
        offset_kernel = "0x00008000";
        offset_ramdisk = "0x01000000";
        offset_second = "0x00000000";
        offset_tags = "0x00000100";
        pagesize = "4096";
      };
    };
    mobile.boot.stage-1 = {
      kernel.package = pkgs.callPackage ./kernel.nix {};
      kernel.modular = true;
      kernel.modules = [ "panel_novatek_nt37705" "spi_geni_qcom" ];
      compression = "zstd";
      firmware = [ (pkgs.runCommand "fp6-initrd-firmware" {} ''
        mkdir -p $out/lib/firmware/postmarketos $out/lib/firmware/qcom/milos/fairphone/fp6
        cp ${config.mobile.device.firmware}/lib/firmware/postmarketos/* $out/lib/firmware/postmarketos/
        cp ${config.mobile.device.firmware}/lib/firmware/postmarketos/* $out/lib/firmware/
        cp ${config.mobile.device.firmware}/lib/firmware/qcom/milos/fairphone/fp6/gen80300_zap.mbn $out/lib/firmware/qcom/milos/fairphone/fp6/
      '') ];
      tasks = [ ./nested-root.rb ];
      # BusyBox cannot set the 4096-byte sector size of userdata's nested GPT.
      extraUtils = lib.mkAfter [{
        package = pkgs.util-linuxMinimal;
        binaries = [ "losetup" ];
      }];
      networking.enable = true;
    };
    mobile.usb = {
      mode = "gadgetfs";
      idVendor = "18d1";
      idProduct = "4ee3";
      gadgetfs.functions.rndis = "ncm.usb0";
      gadgetfs.functions.adb = "ffs.adb";
    };
    hardware.enableRedistributableFirmware = false;
    hardware.firmwareCompression = "none";
    # A separate Android v2 image is repacked from the known working header.
    mobile.quirks.supportsStage-0 = false;
    mobile.outputs.android.android-bootimg = lib.mkForce (pkgs.runCommand "fp6-nixos-boot.img" {
      nativeBuildInputs = [ pkgs.python3 ];
    } ''
      python ${../repack-boot.py} ${../assets/boot-template.img} ${config.mobile.outputs.initrd} $out \
        --cmdline ${lib.escapeShellArg (lib.concatStringsSep " " config.boot.kernelParams)}
    '');
    mobile.outputs.android.android-fastboot-images = lib.mkForce (pkgs.runCommand "fp6-nixos-boot-files" {} ''
      mkdir -p $out
      cp ${config.mobile.outputs.android.android-bootimg} $out/boot.img
      printf '%s\n' 'Test with fastboot boot boot.img. Do not fastboot flash boot or change slots on this FP6.' > $out/INSTALL.txt
    '');
  };
}
