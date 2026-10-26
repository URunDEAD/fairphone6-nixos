# Optional native ARM security tools. Installation never starts tools/services,
# changes interfaces, assigns capabilities, or changes the phone kernel.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.securityTools;
  tools = import ./security-tools-packages.nix { inherit pkgs; };
  listing = pkgs.writeText "fp6-security-tools-list.txt"
    (lib.concatMapStringsSep "\n" (item: "${item.id} ${item.version}: ${lib.concatStringsSep ", " item.bins}") tools.accepted + "\n");
  catalog = pkgs.writeText "fp6-security-tools-catalog.json" (builtins.unsafeDiscardStringContext (builtins.toJSON tools.catalog));
  menu = pkgs.writeShellApplication {
    name = "fp6-security-tools";
    runtimeInputs = [ pkgs.bashInteractive pkgs.coreutils ];
    text = ''
      export PATH=${tools.bundle}/bin:"$PATH"
      if [[ "''${1-}" == --list ]]; then
        cat ${listing}
        exit 0
      fi
      if [[ "''${1-}" == --catalog ]]; then
        cat ${catalog}
        exit 0
      fi
      if [[ "''${1-}" == --help ]]; then
        printf '%s\n' 'Security Tools: an interactive tool-help menu and shell.' \
          '  --list     List installed tool families and versions' \
          '  --catalog  Show package availability metadata'
        exit 0
      fi
      PS3='Choose an action: '
      select action in 'Open shell' 'List tools' 'Tool help' 'Quit'; do
        : "$action"
        case "$REPLY" in
          1)
            printf '%s\n' 'Security Tools shell. Type exit to close.'
            exec ${pkgs.bashInteractive}/bin/bash --noprofile --norc
            ;;
          2) cat ${listing} ;;
          3)
            PS3='Choose a tool, or Back: '
            select tool in ${lib.concatMapStringsSep " " (item: lib.escapeShellArg item.id) tools.accepted} 'Back'; do
              case "$tool" in
                ${lib.concatMapStringsSep "\n" (item:
                  let
                    # CLI help for Wireshark avoids starting its GUI merely to read usage.
                    binary = if item.id == "wireshark" then "tshark" else builtins.head item.bins;
                    flags = if builtins.elem item.id [ "netcat" "metasploit" ] then "-h"
                      else if item.id == "openssh" then "-V" else "--help";
                  in "${lib.escapeShellArg item.id}) ${tools.bundle}/bin/${binary} ${flags} || true ;;"
                ) tools.accepted}
                Back) break ;;
                *) printf '%s\n' 'Choose a listed tool.' ;;
              esac
            done
            PS3='Choose an action: '
            ;;
          4) exit 0 ;;
          *) printf '%s\n' 'Choose an action from 1 to 4.' ;;
        esac
      done
    '';
  };
in {
  options.hardware.fairphone6.securityTools.enable =
    lib.mkEnableOption "the native ARM security tool bundle and terminal command";
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ tools.bundle menu ];
  };
}
