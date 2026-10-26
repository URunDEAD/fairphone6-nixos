{ lib, stdenv, fetchFromGitLab, pkg-config, sqlite, icu }:
stdenv.mkDerivation {
  pname = "sqlite-icu-extension";
  version = "0-unstable-2018-10-16";
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/sqlite-icu-extension";
    rev = "c762fed833417906fb11cb85bb9dfa775068bb36";
    sha256 = "09vab769bvrr0jsgvi2hrn0csc78h5vlkbawlz73j6ajgdd279xc";
  };
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ sqlite icu ];
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    $CC -shared -fPIC ext/icu/icu.c $(pkg-config --cflags --libs sqlite3 icu-uc icu-i18n) -o libSqliteIcu.so
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    result=$(${lib.getExe sqlite} :memory: \
      -cmd ".load $PWD/libSqliteIcu.so sqlite3_icu_init" \
      "SELECT upper('josé')='JOSÉ' AND lower('Ä')='ä';")
    test "$result" = 1
    ${lib.getExe sqlite} :memory: \
      -cmd ".load $PWD/libSqliteIcu.so sqlite3_icu_init" \
      "SELECT icu_load_collation('fi_FI', 'localeCollation');"
    runHook postCheck
  '';
  installPhase = ''
    install -Dm755 libSqliteIcu.so "$out/lib/libSqliteIcu.so"
    install -Dm644 ext/icu/README.txt "$out/share/doc/sqlite-icu-extension/README.txt"
  '';
  meta = {
    description = "SQLite ICU extension from official UBports source, for Unicode contact sorting";
    homepage = "https://gitlab.com/ubports/development/core/sqlite-icu-extension";
    license = lib.licenses.publicDomain;
    platforms = lib.platforms.linux;
  };
}
