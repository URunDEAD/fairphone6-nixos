# CS-call-audio TEST kernel: the exact kernel6 recipe plus the kernel7
# candidates 0005-0008 (q6apm hostless FEs, topology module config,
# qrtr forwarding, topology GRAPH_OPEN extra params). For a supervised RAM
# boot only (results/feature-push-20261024/cs-call-audio/README.md).
# modDirVersion stays 7.2.0-fp6-nixos6: the patches change no exported
# symbol outside snd-q6apm/q6apm-dai (built here) and CONFIG_MODVERSIONS is
# off, so out-of-tree modules keep their vermagic.
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }, system ? "aarch64-linux"
}:
(import ../../kernel6 { inherit nixpkgs system; }).overrideAttrs (old: {
  pname = "linux-fp6-nixos-csvoice";
  patches = (old.patches or [ ]) ++ [
    ../0005-ASoC-qcom-q6apm-dai-support-hostless-front-ends.patch
    ../0006-ASoC-qcom-audioreach-send-topology-module-config-for-any-module.patch
    ../0007-net-qrtr-forward-data-packets-between-endpoints.patch
    ../0008-ASoC-qcom-audioreach-topology-graph-open-extra-params.patch
  ];
})
