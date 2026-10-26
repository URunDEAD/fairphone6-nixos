# Userspace packages for FP6 VoLTE (imsd next to oFono, no ModemManager).
{ pkgs }:
let
  inherit (pkgs) lib;
in rec {
  # `mmcli` answering imsd's four ModemManager queries from oFono + QMI NAS.
  mmcli-ofono-shim-bin = pkgs.writers.writePython3Bin "mmcli" {
    flakeIgnore = [ "E501" ];
  } (builtins.readFile ./mmcli-ofono-shim.py);
  mmcli-ofono-shim = pkgs.runCommand "mmcli-ofono-shim" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    meta.mainProgram = "mmcli";
  } ''
    makeWrapper ${mmcli-ofono-shim-bin}/bin/mmcli $out/bin/mmcli \
      --prefix PATH : ${lib.makeBinPath [ pkgs.systemd pkgs.libqmi pkgs.coreutils ]}
  '';

  # IMS PDN holder: raw QMI WDS (profile-indexed IPv6 call on its own mux).
  fp6-ims-pdn-bin = pkgs.writers.writePython3Bin "fp6-ims-pdn" {
    flakeIgnore = [ "E501" "E731" "E741" "W503" "W504" ];
  } (builtins.readFile ./fp6-ims-pdn.py);
  fp6-ims-pdn = pkgs.runCommand "fp6-ims-pdn" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    meta.mainProgram = "fp6-ims-pdn";
  } ''
    makeWrapper ${fp6-ims-pdn-bin}/bin/fp6-ims-pdn $out/bin/fp6-ims-pdn \
      --prefix PATH : ${lib.makeBinPath [ pkgs.iproute2 pkgs.libqmi pkgs.coreutils ]}
  '';

  # imsd 1.1.0 (the pinned Nura build) with:
  #  * the shim as its `mmcli` (imsd-package.nix puts `modemmanager`'s bin on
  #    every wrapper's PATH, so passing the shim there replaces ModemManager),
  #  * media.role=Phone on the call streams (audio feature push patch),
  #  * the IMSI, own AOR, caller number and SMS body kept out of the journal.
  imsd = (pkgs.callPackage ../../imsd-package.nix {
    modemmanager = mmcli-ofono-shim;
  }).overrideAttrs (old: {
    pname = "imsd-fp6-ofono";
    patches = (old.patches or [ ]) ++ [ ../audio/imsd-media-role-phone.patch ];
    postPatch = (old.postPatch or "") + ''
      substituteInPlace implementations/main.cpp \
        --replace-fail 'Log(std::format("SIM slot={} IMSI={} domain={}", sel->slot, info->imsi, ctx_.id.domain));' \
                       'Log(std::format("SIM slot={} IMSI=<{} digits> domain={}", sel->slot, info->imsi.size(), ctx_.id.domain));' \
        --replace-fail 'Log(std::format("self AOR: {}", ctx_.aor.empty() ? ctx_.id.impu : ctx_.aor));' \
                       'Log(std::format("self AOR: {}", ctx_.aor.empty() ? "(IMPU)" : "(from the network)"));' \
        --replace-fail 'Log(std::format("incoming call {} from {}", uni, call_->Number()));' \
                       'Log(std::format("incoming call {} (caller id {})", uni, call_->Number().empty() ? "absent" : "present"));' \
        --replace-fail 'Log(std::format("MESSAGE from {} ({} bytes):\n{}", imsd::sip::CallerId(msg), body.size(), body));' \
                       'Log(std::format("MESSAGE ({} bytes)", body.size()));'
    '';
  });
}
