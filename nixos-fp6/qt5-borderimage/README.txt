Qt5 BorderImage stale SCI reply fix, pinned package copy.

Expected derivation:
/nix/store/rv55r2scrylkviqsxgf6yaljwqsz019g-qtdeclarative-5.15.19.drv
Expected main output:
/nix/store/849zhwa1cdim0xf4b4yb3x974zrpb891-qtdeclarative-5.15.19

../qt5-borderimage-native.nix overrides the complete qt5 scope and reuses this
exact already-built Qt package. trial-power.nix and trial-kernel-ui.nix import
that module. Dependent Qt modules and apps rebuild with their final native
prefixes. No process library override is added.

Do not change postFixup just to add tests after building: it changes the
package derivation and recompiles Qt. The additional non-null stale-source
regression lives under results/qt-borderimage and runs against the built output.
Source/ABI review, baseline and regression evidence, integration cost and final
closure audits are in ../../results/qt-borderimage/review.txt.

../qt5-borderimage-runtime.nix is a historical diagnostic module, unsuitable
for production. Standard ASCII dependency replacement misses QStringLiteral
UTF-16 resource paths in twelve packages, including keyboard QML/language
paths. Final native roots require complete ELF UTF-16 path checks before
artifact handoff; the earlier rewritten14vm root was not production accepted.
