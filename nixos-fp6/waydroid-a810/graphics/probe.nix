# Native ARM builder only. The raw Android executable never runs during build.
{ pkgs ? import ((import ../../upstream/mobile-nixos/npins).nixpkgs) { system = "aarch64-linux"; } }:
let
  sourcePackage = (import ../default.nix { inherit pkgs; }).source;
in pkgs.stdenvNoCC.mkDerivation {
  pname = "fp6-android-gles-first-draw";
  version = "1";
  outputs = [ "out" "audit" ];
  dontUnpack = true;
  dontFixup = true; # Preserve Bionic interpreter and Android DT_NEEDED names.
  nativeBuildInputs = [ pkgs.python3 pkgs.binutils ];
  buildPhase = ''
    python3 - <<'PY'
    import json,subprocess
    from pathlib import Path
    cfg=json.loads(Path('${sourcePackage}/share/fp6-waydroid-a810/toolchain.json').read_text())
    # Clang21 deprecates absolute -fuse-ld paths; retain the exact linker via
    # its supported spelling, keeping all C warnings enabled and fatal.
    link=[arg.replace('-fuse-ld=/','--ld-path=/',1) for arg in cfg['c_link_args']]
    command=[*cfg['c'],*link,'-Wl,--dynamic-linker,/system/bin/linker64',
             '-std=c11','-O2','-Wall','-Wextra','-Werror',
             '${./gles-first-draw.c}','-lEGL','-lGLESv2','-ldl','-lm','-o','fp6-android-gles-first-draw']
    subprocess.run(command,check=True)
    Path('build-command.json').write_text(json.dumps(command,indent=2)+'\n')
    PY
    readelf -h -l -d fp6-android-gles-first-draw > elf.txt
    grep -q '/system/bin/linker64' elf.txt
    grep -q '\[libEGL.so\]' elf.txt
    grep -q '\[libGLESv2.so\]' elf.txt
  '';
  installPhase = ''
    mkdir -p $out/bin $out/share/fp6-android-gles-first-draw $audit
    cp fp6-android-gles-first-draw $out/bin/
    # Toolchain paths stay on builder; importing the runtime must not pull NDK.
    cp build-command.json elf.txt $audit/
    cp ${./gles-first-draw.c} $out/share/fp6-android-gles-first-draw/source.c
  '';
  passthru.hardwareAcceptance = false;
  meta.platforms = [ "aarch64-linux" ];
}
