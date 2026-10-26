# Original FP6 downstream software ISP/AF/tuning, staged without activation.
{ lib, stdenv, libcamera, fetchgit, libyuv, libjpeg }:
libcamera.overrideAttrs (old: {
  pname = "libcamera-fp6-pmos";
  version = "0.7.2";
  src = fetchgit {
    url = "https://git.libcamera.org/libcamera/libcamera.git";
    rev = "191e202178f02430b5942397c70d215cdd2056fa";
    hash = "sha256-vhFkeT1j2KKm+CVvGrtH5BEYJSEdaX7N7DRdA0a9EWk=";
  };
  patches = (old.patches or [])
    ++ map (name: ./camera-fp6-pmos + "/${name}")
      (lib.filter (name: lib.hasSuffix ".patch" name)
        (builtins.attrNames (builtins.readDir ./camera-fp6-pmos)))
    ++ [ ./camera-fp6-sensor-test.patch ];
  buildInputs = (old.buildInputs or []) ++ [ libyuv libjpeg ];
  mesonFlags = (old.mesonFlags or []) ++ [
    "-Dpipelines=simple,uvcvideo"
    "-Dipas=simple"
    "-Dlibdw=disabled"
    "-Dsoftisp-gpu=disabled"
    "-Dcam-output-sdl2=disabled"
    "-Dapps-output-dng=disabled"
    "-Dtest=true"
  ];
  postInstall = (old.postInstall or "") + ''
    install -Dm644 ${./camera-fp6-pmos/imx896.yaml} \
      "$out/share/libcamera/ipa/simple/imx896.yaml"
  '';
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  checkPhase = ''
    runHook preCheck
    meson test --no-rebuild --print-errorlogs fp6_sensor_helpers
    runHook postCheck
  '';
})
