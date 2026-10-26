{ lib, llvmPackages_21, fetchgit, pkg-config, python3, makeWrapper, glib
, opencore-amr, vo-amrwbenc, pipewire, modemmanager, libqmi, iproute2
, nftables, coreutils, gnugrep, gnused, gawk, systemd, sudo }:
let
  runtimePath = lib.makeBinPath [ modemmanager libqmi iproute2 nftables
    pipewire coreutils gnugrep gnused gawk systemd sudo ];
in llvmPackages_21.stdenv.mkDerivation {
  pname = "imsd";
  version = "1.1.0";
  src = fetchgit {
    url = "https://forgejo.catcrafts.net/Catcrafts/imsd.git";
    rev = "547361a4d476014adccf66170adea19d762bc9e8";
    hash = "sha256-RAiau0rJ6o8BWxNvS7cbckP2T8Sz3sZBWlsGaPZs1gY=";
  };
  nativeBuildInputs = [ pkg-config python3 makeWrapper ];
  buildInputs = [ glib ];
  dontConfigure = true;
  enableParallelBuilding = false;
  postPatch = ''
    substituteInPlace implementations/main.cpp \
      --replace-fail /usr/libexec/imsd-media "$out/libexec/imsd-media"
    substituteInPlace implementations/media.cpp \
      --replace-fail libvo-amrwbenc.so.0 '${lib.getLib vo-amrwbenc}/lib/libvo-amrwbenc.so.0' \
      --replace-fail libopencore-amrnb.so.0 '${lib.getLib opencore-amr}/lib/libopencore-amrnb.so.0' \
      --replace-fail libopencore-amrwb.so.0 '${lib.getLib opencore-amr}/lib/libopencore-amrwb.so.0' \
      --replace-fail '"pw-play"' '"${lib.getExe' pipewire "pw-play"}"' \
      --replace-fail '"pw-record"' '"${lib.getExe' pipewire "pw-record"}"'
    substituteInPlace packaging/ims-pdn-up.sh \
      --replace-fail /usr/libexec/imsd-pcscf "$out/libexec/imsd-pcscf"
  '';
  buildPhase = ''
    runHook preBuild
    python3 ${./imsd-build.py} --test
    runHook postBuild
  '';
  # The build runs upstream's eight pure suites plus the offline media
  # self-test. It never starts a daemon, opens a modem or registers IMS.
  doCheck = false;
  installPhase = ''
    runHook preInstall
    install -Dm755 programs/imsd "$out/bin/imsd"
    install -Dm755 programs/imsd-ofonod "$out/bin/imsd-ofonod"
    install -Dm755 programs/imsd-dialerd "$out/bin/imsd-dialerd"
    install -Dm755 programs/imsd-media "$out/libexec/imsd-media"
    install -Dm755 programs/imsd-pcscf "$out/libexec/imsd-pcscf"
    install -Dm755 packaging/ims-pdn-up.sh "$out/libexec/ims-pdn-up.sh"
    install -Dm755 packaging/imsd-handover.sh "$out/libexec/imsd-handover.sh"
    install -Dm644 packaging/net.catcrafts.IMS1.conf "$out/share/dbus-1/system.d/net.catcrafts.IMS1.conf"
    install -Dm644 packaging/org.ofono.conf "$out/share/dbus-1/system.d/imsd-ofono.conf"
    install -Dm644 packaging/net.catcrafts.IMS1.policy "$out/share/polkit-1/actions/net.catcrafts.IMS1.policy"
    patchShebangs "$out/libexec"
    for program in "$out/bin/imsd" "$out/bin/imsd-ofonod" "$out/bin/imsd-dialerd" "$out/libexec/imsd-media" "$out/libexec/ims-pdn-up.sh" "$out/libexec/imsd-handover.sh"; do
      wrapProgram "$program" --prefix PATH : '${runtimePath}'
    done
    runHook postInstall
  '';
  meta = {
    description = "Catcrafts userspace IMS/VoLTE stack, matching existing FP6 postmarketOS version";
    homepage = "https://forgejo.catcrafts.net/Catcrafts/imsd";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
