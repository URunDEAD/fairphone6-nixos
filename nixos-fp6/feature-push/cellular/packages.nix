# Userspace packages for the FP6 cellular feature push (oFono stack).
{ pkgs }:
let
  inherit (pkgs) lib;
in rec {
  # GNSS NMEA streamer: raw QMI LOC over QRTR (see fp6-loc-nmea.py for why
  # libqmi cannot register on this firmware). Python stdlib only.
  fp6-loc-nmea = pkgs.writers.writePython3Bin "fp6-loc-nmea" {
    flakeIgnore = [ "E501" "E731" "E741" "W503" ];
  } (builtins.readFile ./fp6-loc-nmea.py);

  # Lomiri location indicator / Settings "Location detection" backend.
  fp6-location-indicator-bin = pkgs.writers.writePython3Bin "fp6-location-indicator" {
    libraries = [ pkgs.python3Packages.pygobject3 ];
    flakeIgnore = [ "E501" "W503" "E402" ];
  } (builtins.readFile ./fp6-location-indicator.py);
  fp6-location-indicator = pkgs.runCommand "fp6-location-indicator" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    meta.mainProgram = "fp6-location-indicator";
  } ''
    makeWrapper ${fp6-location-indicator-bin}/bin/fp6-location-indicator $out/bin/fp6-location-indicator \
      --prefix GI_TYPELIB_PATH : ${lib.makeSearchPath "lib/girepository-1.0" [ pkgs.glib.out pkgs.gobject-introspection ]} \
      --prefix PATH : ${lib.makeBinPath [ pkgs.lomiri.lomiri-url-dispatcher ]}
    mkdir -p $out/share/ayatana/indicators
    cat > $out/share/ayatana/indicators/com.lomiri.indicator.location <<EOF
    [Indicator Service]
    Name=fp6-location-indicator
    ObjectPath=/com/lomiri/indicator/location
    Position=60

    [phone]
    ObjectPath=/com/lomiri/indicator/location/phone
    Position=60

    [desktop]
    ObjectPath=/com/lomiri/indicator/location/desktop
    EOF
  '';

  # NetworkManager daemon with upstream's (UBports-contributed) oFono WWAN
  # backend, so oFono ConnectionManager contexts become NM gsm connections
  # (what lomiri-indicator-network expects). Used only as
  # networking.networkmanager.package; libnm consumers keep the stock build.
  networkmanager-ofono = pkgs.networkmanager.overrideAttrs (old: {
    pname = "networkmanager-fp6-ofono";
    mesonFlags = (old.mesonFlags or [ ]) ++ [ "-Dofono=true" ];
    # The test suite is unchanged by the extra backend and is slow on the
    # shared builder; upstream CI covers it.
    doCheck = false;
  });

  msm-modem-uim-selection = pkgs.writeShellApplication {
    name = "msm-modem-uim-selection";
    runtimeInputs = [ pkgs.libqmi pkgs.gnugrep pkgs.gawk pkgs.coreutils pkgs.systemd ];
    text = builtins.readFile ./msm-modem-uim-selection.sh;
  };

  fp6-ofono-account = pkgs.writers.writePython3Bin "fp6-ofono-account" {
    libraries = [ pkgs.python3Packages.pygobject3 ];
    flakeIgnore = [ "E501" "W503" "E402" ];
  } (builtins.readFile ./fp6-ofono-account.py);

  # Wrapper so GI finds Gio typelibs at runtime.
  fp6-ofono-account-wrapped = pkgs.runCommand "fp6-ofono-account-wrapped" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    makeWrapper ${fp6-ofono-account}/bin/fp6-ofono-account $out/bin/fp6-ofono-account \
      --prefix GI_TYPELIB_PATH : ${lib.makeSearchPath "lib/girepository-1.0" [ pkgs.glib.out pkgs.gobject-introspection ]}
  '';

  # linux-msm tqftpserv v1.2 (+ current qrtr from nixpkgs). The Mobile NixOS
  # overlay shadows both with 2020 snapshots, and that tqftpserv ignores the
  # TFTP "rsize"/"seek" options: the FP6 modem reads modem_pr/so/902_0_2.mbn
  # with rsize=52 (ELF header only), the old server streams the whole file,
  # the transfer stalls and the modem never gets to mcfg_hw -> no platform
  # MCFG -> DMS stuck "offline", UIM slots never powered (no-atr-received).
  # nixpkgs' own tqftpserv (1.1-unstable-2025-08-01) handles rsize/seek but
  # stores WRQ uploads as 0-byte files (server_check.txt "hello" and
  # mcfg.tmp arrive empty) and the modem then skips MCFG selection. v1.2 is
  # the version the old postmarketOS root ran, verified on this phone.
  # Built from the nixpkgs package file so the overlay's old qrtr/tqftpserv
  # attributes are bypassed. Firmware is served read-only; /readwrite/
  # (modem scratch: server_check.txt, mcfg.tmp) goes to /run/tqftpserv
  # (tmpfs), which rmtfs wipes on every start (see cellular-fp6.nix).
  qrtr-linux-msm = pkgs.callPackage "${pkgs.path}/pkgs/by-name/qr/qrtr/package.nix" { };
  tqftpserv = (pkgs.callPackage "${pkgs.path}/pkgs/by-name/tq/tqftpserv/package.nix" {
    qrtr = qrtr-linux-msm;
  }).overrideAttrs (old: {
    pname = "tqftpserv-fp6";
    version = "1.2";
    src = pkgs.fetchFromGitHub {
      owner = "linux-msm";
      repo = "tqftpserv";
      rev = "b6bb92d40cfffe28621abcf7bfaa6d99beea46cb"; # v1.2
      hash = "sha256-tXxiunBMbmHcko4tqgMXvOEZrMeqk4quybV1PukW0U0=";
    };
    # Primary lookup uses /sys/module/firmware_class/parameters/path; the
    # fallback base points at the NixOS firmware link.
    postPatch = ''
      substituteInPlace translate.c \
        --replace-fail '"/lib/firmware/"' '"/run/current-system/sw/share/uncompressed-firmware/"' \
        --replace-fail '"/var/lib/tqftpserv"' '"/run/tqftpserv"'
    '';
  });

  # One-shot: activate the modem's auto-selected platform (hardware) MCFG.
  # With rmtfs -r the EFS lives in a RAM shadow, so each boot the modem
  # re-selects mcfg_hw (DSDS_CDMALess-LA-Milos), marks it "Pending" and needs
  # one modem restart to make it "Active". Until then the UIM slots are not
  # configured. This waits for Pending, then restarts the mpss remoteproc
  # ONCE via sysfs (rmtfs keeps running, so the RAM shadow with the selection
  # survives). Only read-only QMI queries (PDC ListConfigs, DMS GetOperatingMode)
  # are sent; no PDC/NV/EFS write is issued by this script.
  fp6-modem-mcfg-activate = pkgs.writeShellApplication {
    name = "fp6-modem-mcfg-activate";
    runtimeInputs = [ pkgs.libqmi pkgs.gnugrep pkgs.gawk pkgs.coreutils ];
    text = builtins.readFile ./fp6-modem-mcfg-activate.sh;
  };

  # System Settings > Cellular panel (network mode, APNs, data roaming).
  lomiri-system-settings-cellular = pkgs.lomiri.callPackage ./lomiri-system-settings-cellular.nix { };

  # IPA data port + QMAP format + rmnet premux links (see the script).
  fp6-modem-data-port = pkgs.writeShellApplication {
    name = "fp6-modem-data-port";
    runtimeInputs = [ pkgs.libqmi pkgs.iproute2 pkgs.kmod pkgs.coreutils ];
    text = builtins.readFile ./fp6-modem-data-port.sh;
  };

  # oFono 2.19 as packaged in nixpkgs has two path bugs that bite here:
  #  * STORAGEDIR is $out/var/lib/ofono (read-only store), so gprs contexts /
  #    APNs, LTE attach settings ("lte_save_settings write_file failed"), SMS
  #    assembly and the SMS tx_queue can never be stored;
  #  * the provisioning DB is looked up only at /usr/share/ofono/provision.db
  #    ("Unable to open provisioning database!", "Provisioning failed"), while
  #    the build installs it to $out/share/ofono/provision.db.
  #
  # FP6 patches (results/feature-push-20261024/cellular/4g5g-README.md):
  #  * 0001: 5G NR. RadioSettings "nr" (TechnologyPreference values name the
  #    highest generation, as Ubuntu Touch / lomiri-system-settings-cellular
  #    expect: "lte" = 2G/3G/4G, "nr" = 2G/3G/4G/5G); QMI mode preference
  #    changes are sent with Change Duration = power-cycle (no NV write);
  #    NetworkRegistration.Technology / ConnectionManager.Bearer "nr" for
  #    NR SA and for LTE cells with EN-DC available and DCNR not restricted.
  #  * 0002: mobile data off (ConnectionManager.Powered=false) keeps the PS
  #    attach (modem stays on LTE, SMS over SGs and IMS unaffected) and only
  #    releases the non-IMS contexts; data on re-reports the LTE default
  #    bearer (qmimodem list_active_contexts).
  #  * 0003: re-apply TechnologyPreference whenever the modem goes online
  #    (QMI falls back to the stored NV mode preference on LPM -> online).
  #  * 0004: Modem.Serial "unknown" when DMS reports no IMEI/ESN/MEID (this
  #    modem); lomiri-indicator-network needs a Serial to use the modem.
  ofono = pkgs.ofono.overrideAttrs (old: {
    pname = "ofono-fp6";
    patches = (old.patches or [ ]) ++ [
      ./ofono-0001-nr-5g.patch
      ./ofono-0002-keep-ps-attach-when-data-off.patch
      ./ofono-0003-radio-settings-reapply-on-online.patch
      ./ofono-0004-qmimodem-serial-placeholder.patch
    ];
    configureFlags = (old.configureFlags or [ ]) ++ [ "--localstatedir=/var" ];
    postPatch = (old.postPatch or "") + ''
      substituteInPlace src/provisiondb.c \
        --replace-fail '"/usr/share/ofono/provision.db"' "\"$out/share/ofono/provision.db\""
    '';
  });
}
