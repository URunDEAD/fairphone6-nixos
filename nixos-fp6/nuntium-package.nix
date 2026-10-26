{ lib, buildGoModule, fetchFromGitLab }:
buildGoModule {
  pname = "nuntium";
  version = "2.2-unstable-2026-08-27";
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/nuntium";
    rev = "d051757417a42872abb99c7583a8a4af6e3108f5";
    sha256 = "1i283ixz6ids139qi5lkfxh8w9jpdm3wj0xw3k3gd477pzrczlg9";
  };
  patches = [ ./nuntium-go-ldm-update.patch ./nuntium-optional-preferred.patch ];
  vendorHash = "sha256-UrcepOFO++z1AbtAKrVhYza46ypc1lbGYlh6w0TPRf8=";
  subPackages = [ "cmd/nuntium" "cmd/nuntium-decode-cli" "cmd/nuntium-inject-push" ];
  env = {
    CGO_ENABLED = "0";
    GOMAXPROCS = "2";
  };
  preBuild = ''
    export GOFLAGS="$GOFLAGS -p=2"
  '';
  # All upstream tests are pure parsing, encoding, context selection and
  # synthetic data tests. No daemon main function or radio service is run.
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export GOFLAGS=''${GOFLAGS//-trimpath/}
    # The default builder follows subPackages, which would omit the protocol
    # libraries' tests. Exercise every upstream package explicitly instead.
    go test -p=2 -timeout=60s -v ./...
    runHook postCheck
  '';
  postInstall = ''
    install -Dm644 README.md "$out/share/doc/nuntium/README.md"
    install -Dm644 docs/architecture.md "$out/share/doc/nuntium/architecture.md"
    install -Dm644 docs/testing.md "$out/share/doc/nuntium/testing.md"
  '';
  meta = {
    description = "UBports MMS daemon between oFono push/contexts, Lomiri transfers and Telepathy";
    homepage = "https://gitlab.com/ubports/development/core/nuntium";
    license = lib.licenses.gpl3;
    platforms = lib.platforms.linux;
    mainProgram = "nuntium";
  };
}
