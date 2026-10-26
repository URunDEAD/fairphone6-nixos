# Compile only the preserved-symbol front-camera DTB, never the kernel Image.
{ runCommand, kernel }:
assert kernel.drvPath == "/nix/store/cd6wlg7kn4zmh87wkywan6zkwk3cq1a7-linux-fp6-nixos-7.2.0.drv";
assert kernel.modDirVersion == "7.2.0-fp6-nixos3";
assert builtins.hashFile "sha256" ./selfie-fp6-boot-dtb/candidate-preserved.pp.dts == "124f95cdfcb1982eec22498a64c09f70e5544919f7944eacd39977730ebce6c4";
runCommand "fp6-selfie-preserved-symbols.dtb" {} ''
  headers=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build
  echo 'b5f594482f69fdc743d3e66c755340f01a666050c432c8454eff2dd6943bf4f8  ${kernel}/dtbs/qcom/milos-fairphone-fp6.dtb' | sha256sum -c -
  "$headers/scripts/dtc/dtc" -b 0 -@ \
    -Wno-unique_unit_address -Wno-unit_address_vs_reg \
    -Wno-avoid_unnecessary_addr_size -Wno-alias_paths \
    -Wno-interrupt_map -Wno-simple_bus_reg \
    -I dts -O dtb -o "$out" ${./selfie-fp6-boot-dtb/candidate-preserved.pp.dts}
  echo '93bfe406603df92627f43d9740b201d6b5781da40773105c627ccdd6580e65cc  '"$out" | sha256sum -c -
''
