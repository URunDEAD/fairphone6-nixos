{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fp6BluetoothIdentity;
  setup = pkgs.writeShellApplication {
    name = "fp6-bluetooth-identity";
    runtimeInputs = [ pkgs.coreutils pkgs.gnugrep pkgs.gnused pkgs.bluez ];
    text = ''
      fail() { echo "FP6 Bluetooth identity: $*" >&2; exit 1; }
      grep -aq 'fairphone,fp6' /sys/firmware/devicetree/base/compatible || fail 'wrong board'

      # Reproduce the original postmarketOS bootmac SERIAL_NUMBER input:
      # its grep deliberately retains the literal "serialno=" prefix.
      serial_token=$(grep -o 'serialno.*' /proc/cmdline | cut -d ' ' -f1)
      [[ "$serial_token" =~ ^serialno=[0-9a-f]+$ ]] || fail 'missing or ambiguous bootloader serial'
      serial_value="''${serial_token#*=}"
      soc_decimal=$(cat /sys/devices/soc0/serial_number)
      [[ "$soc_decimal" =~ ^[0-9]+$ ]] || fail 'unexpected SoC serial format'
      soc_hex=$(printf '%x' "$soc_decimal")
      [[ "$serial_value" == "$soc_hex" ]] || fail 'bootloader and hardware serial mismatch'

      hash_prefix=$(printf '%s-BT\n' "$serial_token" | sha256sum | cut -c1-8)
      bt_address=$(printf '0200%s\n' "$hash_prefix" | sed 's/\(..\)/\1:/g;s/:$//')
      [[ "$bt_address" =~ ^02:00:([0-9a-f]{2}:){3}[0-9a-f]{2}$ ]] || fail 'invalid generated identity'

      # Preserve a previously used identity, rather than silently assigning
      # a new truncated-hash address. Never read pairing keys or copy state.
      previous=${lib.escapeShellArg cfg.previousStateDirectory}
      [[ -d "$previous" && ! -L "$previous" ]] || fail 'prior Bluetooth state unavailable'
      matches=0
      candidates=0
      for entry in "$previous"/*; do
        [[ -d "$entry" && ! -L "$entry" ]] || continue
        name="''${entry##*/}"
        [[ "$name" =~ ^([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}$ ]] || continue
        candidates=$((candidates + 1))
        [[ "''${name,,}" != "$bt_address" ]] || matches=$((matches + 1))
      done
      [[ "$candidates" == 1 && "$matches" == 1 ]] || fail 'prior identity mismatch or ambiguity'

      # A local collision is detectable; global uniqueness of the legacy
      # 32-bit truncated hash cannot be guaranteed by this helper.
      if [[ -r /sys/class/net/wlan0/address ]]; then
        wifi_address=$(cat /sys/class/net/wlan0/address)
        [[ "''${wifi_address,,}" != "$bt_address" ]] || fail 'address collides with Wi-Fi'
      fi
      for adapter in /sys/class/bluetooth/hci*; do
        [[ "''${adapter##*/}" == hci0 ]] || fail 'unexpected additional Bluetooth controller'
      done
      [[ -d /sys/class/bluetooth/hci0 ]] || fail 'hci0 absent'

      # Be idempotent, and refuse to replace another configured identity.
      current_info=$(timeout 5 btmgmt -i 0 info 2>/dev/null || true)
      current_address=$(printf '%s\n' "$current_info" | sed -nE 's/^[[:space:]]*addr ([0-9a-fA-F:]{17}).*/\1/p')
      if [[ -n "$current_address" ]]; then
        [[ "''${current_address,,}" == "$bt_address" ]] || fail 'controller already has another configured identity'
        echo 'FP6 Bluetooth identity: verified prior identity already configured'
        exit 0
      fi
      # hci0 appears before QCA firmware setup necessarily finishes.
      # Retry read-only configuration queries for at most ~20 seconds.
      configuration=""
      for _ in 1 2 3 4 5; do
        configuration=$(timeout 3 btmgmt -i 0 config 2>/dev/null || true)
        if printf '%s\n' "$configuration" | grep -q 'missing options:'; then
          break
        fi
        sleep 1
      done
      printf '%s\n' "$configuration" | grep -qE '^[[:space:]]*missing options: public-address[[:space:]]*$' || fail 'unexpected or unavailable controller configuration requirements'

      # Only supply the missing management public-address option. No power,
      # rfkill, hciconfig-up/down, Wi-Fi or persistent calibration writes.
      # Suppress btmgmt output because it includes the address.
      timeout 15 btmgmt -i 0 public-addr "$bt_address" >/dev/null 2>&1 || fail 'management address setup failed'
      # Setting the address queues asynchronous QCA setup; the command's
      # success alone does not establish a configured controller yet.
      for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do
        current_info=$(timeout 2 btmgmt -i 0 info 2>/dev/null || true)
        current_address=$(printf '%s\n' "$current_info" | sed -nE 's/^[[:space:]]*addr ([0-9a-fA-F:]{17}).*/\1/p')
        if [[ -n "$current_address" ]]; then
          [[ "''${current_address,,}" == "$bt_address" ]] || fail 'configured controller identity differs from verified prior identity'
          echo 'FP6 Bluetooth identity: verified prior identity restored and controller ready'
          exit 0
        fi
        sleep 1
      done
      fail 'controller did not become configured within readiness bound'
    '';
  };
in {
  options.hardware.fp6BluetoothIdentity = {
    enable = lib.mkEnableOption "restore the verified previous postmarketOS Bluetooth identity on FP6";
    previousStateDirectory = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/var/lib/bluetooth";
      description = "Existing BlueZ state directory used only to verify the prior address directory name.";
    };
  };
  config = lib.mkIf cfg.enable {
    system.build.fp6-bluetooth-identity = setup;
    hardware.bluetooth.enable = true;
    systemd.services.fp6-bluetooth-identity = {
      description = "Restore verified FP6 Bluetooth identity";
      wantedBy = [ "multi-user.target" ];
      after = [ "sys-subsystem-bluetooth-devices-hci0.device" ];
      bindsTo = [ "sys-subsystem-bluetooth-devices-hci0.device" ];
      before = [ "bluetooth.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = lib.getExe setup;
        TimeoutStartSec = 85;
        CapabilityBoundingSet = [ "CAP_NET_ADMIN" "CAP_NET_RAW" ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ReadOnlyPaths = [ cfg.previousStateDirectory ];
        RestrictAddressFamilies = [ "AF_UNIX" "AF_BLUETOOTH" ];
      };
    };
    systemd.services.bluetooth = {
      requires = [ "fp6-bluetooth-identity.service" ];
      after = [ "fp6-bluetooth-identity.service" ];
    };
  };
}
