# FP6 VoLTE: Catcrafts imsd (userspace IMS: SIP, IPsec, AKA, AMR RTP via
# PipeWire) running next to oFono, without ModemManager.
#
#   fp6-ims-pdn.service  holds the carrier IMS PDN: one raw QMI WDS client,
#                        profile-indexed IPv6 call on its own rmnet link
#                        (rmnet_ims, mux 8). oFono's own "ims" context cannot
#                        be used: its qmimodem driver starts calls by APN
#                        string, which this modem refuses for IPv6 while the
#                        attach PDN is IPv4-only (internal/210).
#                        Writes /run/imsd.env (DEV, LOCAL, P-CSCF from the PCO).
#   imsd.service         imsd 1.1.0 with an oFono-backed `mmcli` shim.
#
# Runtime-only modem use: Start/Stop Network on an existing carrier profile.
# No profile, NV, EFS, PDC or IMEI write. The modem's own IMS stack is not used.
#
# autoStart stays false until Lomiri can show IMS calls: an IMS-registered
# phone receives its calls on imsd (net.catcrafts.IMS1), not on oFono, and
# telepathy-ofono / the dialer would not ring. See
# results/feature-push-20261024/ims/README.md.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6Ims;
  fp6 = import ./packages.nix { inherit pkgs; };
in {
  options.services.fp6Ims = {
    enable = lib.mkEnableOption "VoLTE through imsd next to oFono (units installed, started on demand)";
    autoStart = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Start the IMS PDN and imsd with oFono at boot (registers IMS automatically).";
    };
    mux = lib.mkOption {
      type = lib.types.ints.between 5 254;
      default = 8;
      description = "rmnet mux id of the IMS link; oFono's premux links use 1..4.";
    };
    interface = lib.mkOption {
      type = lib.types.str;
      default = "rmnet_ims";
    };
    apn = lib.mkOption {
      type = lib.types.str;
      default = "ims";
      description = "IMS APN; the modem profile with this APN is used by index.";
    };
    sendImei = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Put the IMEI URN in imsd's +sip.instance (what the stock modem sends).
        Off by default; enable only if the carrier refuses registration without it.
      '';
    };
    package = lib.mkOption {
      type = lib.types.package;
      default = fp6.imsd;
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = config.services.ofono.enable or false;
        message = "fp6Ims runs next to oFono (services.fp6Cellular)."; }
      { assertion = !(config.networking.modemmanager.enable or false);
        message = "fp6Ims replaces ModemManager for imsd; do not enable both."; }
    ];

    environment.systemPackages = [ cfg.package fp6.fp6-ims-pdn ];
    # net.catcrafts.IMS1 bus policy + polkit action (call control: active
    # local session and root).
    services.dbus.packages = [ cfg.package ];
    security.polkit.enable = true;

    system.build.fp6-ims-imsd = cfg.package;
    system.build.fp6-ims-pdn = fp6.fp6-ims-pdn;
    system.build.fp6-ims-mmcli-shim = fp6.mmcli-ofono-shim;

    systemd.services.fp6-ims-pdn = {
      description = "FP6 carrier IMS PDN for imsd (QMI WDS, profile-indexed IPv6)";
      after = [ "ofono.service" ];
      bindsTo = [ "ofono.service" ];
      wantedBy = lib.optional cfg.autoStart "ofono.service";
      path = [ pkgs.iproute2 pkgs.libqmi pkgs.coreutils pkgs.systemd ];
      # LTE + PS attach are needed: with mobile data off oFono sets the
      # service domain to cs-only and the modem leaves LTE.
      preStart = ''
        for _ in $(seq 1 60); do
          st=$(${fp6.mmcli-ofono-shim}/bin/mmcli -m any -K 2>/dev/null || true)
          case "$st" in *"packet-service-state"*": attached"*) exit 0 ;; esac
          sleep 5
        done
        echo "fp6-ims-pdn: no PS attach after 300 s (mobile data off or no LTE)"
        exit 1
      '';
      serviceConfig = {
        Type = "notify";
        NotifyAccess = "main";
        ExecStart = "${fp6.fp6-ims-pdn}/bin/fp6-ims-pdn --dev ${cfg.interface} --mux ${toString cfg.mux} --apn ${cfg.apn}";
        ExecStopPost = "-${pkgs.iproute2}/bin/ip link del ${cfg.interface}";
        TimeoutStartSec = 420;
        Restart = "on-failure";
        RestartSec = 30;
      };
      startLimitIntervalSec = 1800;
      startLimitBurst = 10;
    };

    systemd.services.imsd = {
      description = "Userspace IMS/VoLTE daemon (imsd) over the FP6 IMS PDN";
      after = [ "fp6-ims-pdn.service" ];
      requires = [ "fp6-ims-pdn.service" ];
      partOf = [ "fp6-ims-pdn.service" ];
      wantedBy = lib.optional cfg.autoStart "fp6-ims-pdn.service";
      environment = {
        IMSD_MEDIA = "${cfg.package}/libexec/imsd-media";
        DUMP_SIP = "0";
        RTP_DUMP = "0";
        PCM_DUMP = "0";
      } // lib.optionalAttrs cfg.sendImei { FP6_IMS_SEND_IMEI = "1"; };
      serviceConfig = {
        Type = "simple";
        EnvironmentFile = [ "-/run/imsd.env" "-/etc/imsd.env" ];
        ExecStart = "${cfg.package}/bin/imsd";
        StateDirectory = "imsd";
        StateDirectoryMode = "0700";
        Restart = "on-failure";
        RestartSec = 120;
        KillMode = "mixed";
        TimeoutStopSec = 10;
      };
    };

    # Protected SIP ports (network-initiated NOTIFY / MT INVITE after ESP
    # decap) and RTP, on the IMS link only.
    networking.firewall.interfaces.${cfg.interface} = {
      allowedTCPPorts = [ 45061 45062 ];
      allowedUDPPorts = [ 45061 45062 50004 ];
    };
    networking.firewall.extraInputRules = lib.mkIf config.networking.nftables.enable ''
      iifname "${cfg.interface}" meta l4proto esp accept comment "fp6-ims ESP"
    '';
  };
}
