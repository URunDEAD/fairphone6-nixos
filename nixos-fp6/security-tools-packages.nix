# Native Linux tools only; no Android overlay, service, capabilities or kernel changes.
{ pkgs }:
let
  inherit (pkgs) lib;
  specs = [
    { id = "nmap"; attr = [ "nmap" ]; bins = [ "nmap" "ncat" ]; }
    { id = "tcpdump"; attr = [ "tcpdump" ]; bins = [ "tcpdump" ]; }
    { id = "wireshark"; attr = [ "wireshark" ]; bins = [ "wireshark" "tshark" "dumpcap" ]; }
    { id = "aircrack-ng"; attr = [ "aircrack-ng" ]; bins = [ "aircrack-ng" "airmon-ng" ]; }
    { id = "kismet"; attr = [ "kismet" ]; bins = [ "kismet" ]; }
    { id = "bettercap"; attr = [ "bettercap" ]; bins = [ "bettercap" ]; }
    { id = "mitmproxy"; attr = [ "mitmproxy" ]; bins = [ "mitmproxy" "mitmdump" "mitmweb" ]; }
    { id = "metasploit"; attr = [ "metasploit" ]; bins = [ "msfconsole" "msfvenom" ]; }
    { id = "sqlmap"; attr = [ "sqlmap" ]; bins = [ "sqlmap" ]; }
    { id = "nikto"; attr = [ "nikto" ]; bins = [ "nikto" ]; }
    { id = "ffuf"; attr = [ "ffuf" ]; bins = [ "ffuf" ]; }
    { id = "gobuster"; attr = [ "gobuster" ]; bins = [ "gobuster" ]; }
    { id = "john"; attr = [ "john" ]; bins = [ "john" ]; }
    { id = "hashcat"; attr = [ "hashcat" ]; bins = [ "hashcat" ]; }
    # pkgs.hydra is the Nix CI server; thc-hydra supplies the requested CLI.
    { id = "hydra"; attr = [ "thc-hydra" ]; bins = [ "hydra" ]; }
    { id = "radare2"; attr = [ "radare2" ]; bins = [ "radare2" "r2" ]; }
    { id = "binwalk"; attr = [ "binwalk" ]; bins = [ "binwalk" ]; }
    { id = "apktool"; attr = [ "apktool" ]; bins = [ "apktool" ]; }
    { id = "jadx"; attr = [ "jadx" ]; bins = [ "jadx" ]; }
    { id = "netcat"; attr = [ "netcat-openbsd" ]; bins = [ "nc" ]; }
    { id = "socat"; attr = [ "socat" ]; bins = [ "socat" ]; }
    { id = "ethtool"; attr = [ "ethtool" ]; bins = [ "ethtool" ]; }
    { id = "iw"; attr = [ "iw" ]; bins = [ "iw" ]; }
    { id = "dnsutils"; attr = [ "dnsutils" ]; bins = [ "dig" "nslookup" ]; }
    { id = "mtr"; attr = [ "mtr" ]; bins = [ "mtr" ]; }
    { id = "traceroute"; attr = [ "traceroute" ]; bins = [ "traceroute" ]; }
    { id = "curl"; attr = [ "curl" ]; bins = [ "curl" ]; }
    { id = "wget"; attr = [ "wget" ]; bins = [ "wget" ]; }
    { id = "openssh"; attr = [ "openssh" ]; bins = [ "ssh" "scp" "sftp" ]; }
    { id = "usbutils"; attr = [ "usbutils" ]; bins = [ "lsusb" ]; }
    { id = "minicom"; attr = [ "minicom" ]; bins = [ "minicom" ]; }
  ];
  inspect = spec:
    let
      package = lib.attrByPath spec.attr null pkgs;
      exists = package != null;
      meta = if exists then package.meta or {} else {};
      licenses = lib.toList (meta.license or []);
      free = exists && licenses != [] && lib.all (license: license.free or false) licenses;
      platform = exists && lib.meta.availableOn pkgs.stdenv.hostPlatform package;
      vulnerable = meta.knownVulnerabilities or [];
      broken = meta.broken or false;
      checked = if exists && free && platform && !broken && vulnerable == []
        then builtins.tryEval (builtins.deepSeq package.drvPath package.drvPath)
        else { success = false; value = false; };
    in spec // {
      inherit exists free platform vulnerable broken;
      version = if exists then package.version or "" else "";
      licenses = map (license: license.spdxId or license.shortName or "unknown") licenses;
      eligible = checked.success;
      drvPath = if checked.success then checked.value else null;
    };
  catalog = map inspect specs;
  accepted = builtins.filter (item: item.eligible) catalog;
  packages = map (item: lib.getAttrFromPath item.attr pkgs) accepted;
in {
  inherit catalog accepted packages;
  blocked = builtins.filter (item: !item.eligible) catalog;
  # Bin/man links only: package-provided service/autostart trees are not exposed.
  bundle = pkgs.buildEnv {
    name = "fp6-native-security-tools";
    paths = packages;
    pathsToLink = [ "/bin" "/share/man" ];
    ignoreCollisions = true;
  };
}
