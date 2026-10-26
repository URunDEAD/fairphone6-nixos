# lpac (eSIM LPA client) with the QMI-over-QRTR APDU driver. nixpkgs builds
# only the char-device `qmi` driver; the FP6 modem is reached over QRTR
# (no /dev/cdc-wdm*), so eSIM profiles need LPAC_APDU=qmi_qrtr.
{ lpac, libqmi, libqrtr-glib, lib }:
(lpac.override { libqmi = libqmi; }).overrideAttrs (old: {
  pname = "lpac-qrtr";
  cmakeFlags = (old.cmakeFlags or [ ]) ++ [ (lib.cmakeBool "LPAC_WITH_APDU_QMI_QRTR" true) ];
  buildInputs = (old.buildInputs or [ ]) ++ [ libqrtr-glib ];
})
