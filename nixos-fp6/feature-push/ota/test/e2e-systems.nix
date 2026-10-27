# Laptop (x86) dry run: small fake "systems" shaped like the real ones.
# For release N: a toplevel named ...-nixos-system-fp6-ota-test-rN with
# etc -> a separate store path holding fp6-release (as NixOS does), a
# payload dependency (so the closure has several paths), and a v2 Android
# boot image whose command line pins the toplevel (mobile-nixos.generation=).
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { }, release ? 2, salt ? "" }:
let
  r = toString release;
  payload = pkgs.runCommand "fp6-ota-test-payload-r${r}" { } ''
    mkdir -p $out
    head -c 300000 /dev/zero | tr '\0' 'r' > $out/data
    echo "${r}${salt}" >> $out/data
  '';
  etc = pkgs.runCommand "etc-fp6-ota-test-r${r}" { } ''
    mkdir -p $out/etc
    cat > $out/etc/fp6-release <<JSON
    {"device":"fairphone-fp6","channel":"test","release":${r},"version":"FP6 r${r} (ota-test)","label":"ota-test-r${r}","kernel":"7.2.0-test"}
    JSON
  '';
  toplevel = pkgs.runCommand "nixos-system-fp6-ota-test-r${r}" { } ''
    mkdir -p $out
    ln -s ${etc}/etc $out/etc
    ln -s ${payload} $out/payload
    printf '#!/bin/sh\necho fake init r${r}\n' > $out/init
    chmod +x $out/init
  '';
  bootimg = pkgs.runCommand "fp6-nixos-boot-ota-test-r${r}.img" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    python3 - ${toplevel} $out <<'PY'
    import struct, sys
    top, out = sys.argv[1], sys.argv[2]
    page = 4096
    kernel = bytes(range(256)) * 40      # 10240 B
    ramdisk = b"ramdisk-${r}" * 900
    dtb = b"\xd0\x0d\xfe\xed" + b"\0" * 1000
    cmd = ("console=tty1 quiet mobile-nixos.generation=" + top).encode()
    h = bytearray(page)
    h[0:8] = b"ANDROID!"
    struct.pack_into("<I", h, 8, len(kernel)); struct.pack_into("<I", h, 16, len(ramdisk))
    struct.pack_into("<I", h, 36, page); struct.pack_into("<I", h, 40, 2)
    h[64:64 + min(len(cmd), 512)] = cmd[:512]
    if len(cmd) > 512: h[608:608 + len(cmd) - 512] = cmd[512:]
    struct.pack_into("<I", h, 1648, len(dtb))
    pad = lambda b: b + b"\0" * (-len(b) % page)
    open(out, "wb").write(bytes(h) + pad(kernel) + pad(ramdisk) + pad(dtb))
    PY
  '';
in { inherit toplevel bootimg payload etc; }
