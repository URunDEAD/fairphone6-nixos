# Interim verified packaging, NOT a claim of a hermetic source rebuild.
{ lib, stdenvNoCC, python3, binutils, archive }:
let
  provenance = builtins.fromJSON (builtins.readFile ./provenance.json);
in
assert builtins.hashFile "sha256" archive == provenance.archive_sha256;
stdenvNoCC.mkDerivation {
  pname = "fp6-waydroid-mesa-a810-binary";
  version = "26.1.4";
  src = archive;
  nativeBuildInputs = [ python3 binutils ];
  dontUnpack = true;
  dontFixup = true; # Never patch/strip Bionic objects as host Linux libraries.
  installPhase = ''
    runHook preInstall
    mkdir -p $out
    python3 - "$src" "$out" <<'PY'
    import pathlib,sys,tarfile
    destination=pathlib.Path(sys.argv[2])
    with tarfile.open(sys.argv[1]) as archive:
      for entry in archive:
        path=pathlib.PurePosixPath(entry.name)
        assert not path.is_absolute() and '..' not in path.parts
        if entry.isdir(): continue
        assert entry.isfile() and str(path).startswith('lib64/'), entry.name
        target=destination/path
        target.parent.mkdir(parents=True,exist_ok=True)
        with archive.extractfile(entry) as source,target.open('wb') as output:
          output.write(source.read())
        target.chmod(0o644)
    PY
    mkdir -p $out/share/fp6-waydroid-a810
    python3 ${./validate-overlay.py} $out/lib64 \
      --expected ${./artifact-hashes.json} --consumer-imports ${./consumer-imports.json} \
      --output $out/share/fp6-waydroid-a810/artifacts.json
    cp ${./provenance.json} $out/share/fp6-waydroid-a810/provenance.json
    runHook postInstall
  '';
  passthru = {
    inherit provenance;
    isBinaryRepack = true;
    hardwareAcceptance = false;
  };
  meta = {
    description = "Pinned seven-library Bionic Mesa A810 overlay for isolated FP6 trials";
    license = with lib.licenses; [ mit bsd2 asl20 ];
    platforms = lib.platforms.linux;
  };
}
