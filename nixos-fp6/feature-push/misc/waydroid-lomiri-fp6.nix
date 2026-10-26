# Waydroid display/suspend settings for the FP6 Lomiri session (Waydroid 1.6.3,
# LineageOS 20 image). Notes: results/feature-push-20261024/waydroid/README.md
#
# 1. Display size. The Waydroid hwcomposer takes persist.waydroid.width/height
#    in Wayland *logical* units and multiplies them by the wl_output scale (3
#    on the FP6). Without them, under Mir it came up at 3348x7236 (1116x2412
#    logical x 3): 9x the pixels, ~290 MB of gralloc buffers instead of ~32 MB.
#    That is the likely trigger for the Lomiri glyph-cache "white boxes".
#    372x804 logical gives 1116x2412 physical, the same as the panel.
#    Verified headless: wm size 1116x2412, SurfaceFlinger 1116x2412, buffer
#    allocation 32 MB.
# 2. Density. waydroid session start sets ro.sf.lcd_density = GRID_UNIT_PX*20,
#    and that value overrides waydroid_base.prop. The Lomiri shell has
#    GRID_UNIT_PX=24 (480 dpi), but apps started from Lomiri get GRID_UNIT_PX=8
#    (160 dpi), so the density depended on where Waydroid was started from.
#    The waydroid CLI is wrapped to pin GRID_UNIT_PX = density/20.
# 3. Auto-freeze. With persist.waydroid.suspend unset, Android "sleeps" a few
#    seconds after boot when no app is in front. IHardware.suspend() then runs
#    lxc-freeze (suspend_action=freeze). On this kernel the cgroup2 freeze never
#    finishes: a thread blocked in fuse_chan_send waits on the already-frozen
#    Android FUSE daemon, so the container stays FREEZING. waydroid's
#    unfreeze() only acts on FROZEN, so app launch / prop set then block on
#    binder forever. persist.waydroid.suspend=false stops Android requesting it.
#
# Mechanism: waydroid writes waydroid_base.prop only at init/upgrade, from
# auto-detected values plus waydroid.cfg [properties]. Each session's
# /vendor/waydroid.prop = waydroid_base.prop + session props. A oneshot that
# runs before waydroid-container.service upserts the keys into both files, so
# they apply to existing installs and survive a re-init. It does nothing if
# Waydroid was never initialized. Runtime `waydroid prop set` values are
# persisted in Android /data and still take precedence.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.waydroidLomiri;
  props = {
    "persist.waydroid.width" = toString cfg.logicalWidth;
    "persist.waydroid.height" = toString cfg.logicalHeight;
    "persist.waydroid.suspend" = lib.boolToString cfg.allowSuspend;
    "persist.waydroid.multi_windows" = lib.boolToString cfg.multiWindows;
    "ro.sf.lcd_density" = toString cfg.density;
  } // cfg.extraProperties;
  propsJson = pkgs.writeText "fp6-waydroid-props.json" (builtins.toJSON props);
  applyProps = pkgs.writers.writePython3 "fp6-waydroid-apply-props" { } ''
    import configparser
    import json
    import os
    import sys

    work = "/var/lib/waydroid"
    cfg_path = os.path.join(work, "waydroid.cfg")
    base_path = os.path.join(work, "waydroid_base.prop")
    with open(sys.argv[1]) as f:
        props = json.load(f)

    if not os.path.isfile(cfg_path):
        print("waydroid not initialized; nothing to do")
        sys.exit(0)

    # waydroid.cfg is read and written by waydroid with configparser too.
    cfg = configparser.ConfigParser()
    cfg.optionxform = str
    cfg.read(cfg_path)
    if not cfg.has_section("properties"):
        cfg.add_section("properties")
    changed = False
    for k, v in props.items():
        if cfg["properties"].get(k) != v:
            cfg["properties"][k] = v
            changed = True
    if changed:
        tmp = cfg_path + ".fp6-tmp"
        with open(tmp, "w") as f:
            cfg.write(f)
        os.chmod(tmp, 0o644)
        os.replace(tmp, cfg_path)

    if os.path.isfile(base_path):
        with open(base_path) as f:
            lines = [ln.rstrip("\n") for ln in f if ln.strip()]
        keep = [ln for ln in lines if ln.split("=", 1)[0] not in props]
        new = keep + [k + "=" + v for k, v in props.items()]
        if new != lines:
            tmp = base_path + ".fp6-tmp"
            with open(tmp, "w") as f:
                f.writelines(ln + "\n" for ln in new)
            os.chmod(tmp, 0o644)
            os.replace(tmp, base_path)
            changed = True
    print("updated" if changed else "already up to date",
          json.dumps(props, sort_keys=True))
  '';
  # Pins the density that `waydroid session start` derives from GRID_UNIT_PX.
  wrappedWaydroid = pkgs.symlinkJoin {
    name = "waydroid-fp6-lomiri-${cfg.basePackage.version}";
    paths = [ cfg.basePackage ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      wrapProgram $out/bin/waydroid --set GRID_UNIT_PX ${toString (cfg.density / 20)}
    '';
    inherit (cfg.basePackage) meta;
    passthru = (cfg.basePackage.passthru or { }) // { inherit (cfg.basePackage) version; };
  };
in {
  options.hardware.fairphone6.waydroidLomiri = {
    enable = lib.mkEnableOption "FP6 Waydroid display size, density and suspend settings for Lomiri";
    basePackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.waydroid;
      defaultText = lib.literalExpression "pkgs.waydroid";
      description = "Waydroid package that is wrapped to pin GRID_UNIT_PX.";
    };
    logicalWidth = lib.mkOption {
      type = lib.types.ints.positive;
      default = 372;
      description = "persist.waydroid.width in Wayland logical px (x scale 3 = 1116 physical).";
    };
    logicalHeight = lib.mkOption {
      type = lib.types.ints.positive;
      default = 804;
      description = "persist.waydroid.height in Wayland logical px (x scale 3 = 2412 physical).";
    };
    density = lib.mkOption {
      type = lib.types.ints.positive;
      default = 480;
      description = "Android density (ro.sf.lcd_density); must be a multiple of 20 (GRID_UNIT_PX = density/20).";
    };
    allowSuspend = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        persist.waydroid.suspend. If true, Android asks Waydroid to freeze
        the container when idle. On the FP6 that freeze hangs in FREEZING
        (FUSE deadlock).
      '';
    };
    multiWindows = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "persist.waydroid.multi_windows (false = one full-UI window).";
    };
    extraProperties = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Additional Android properties for waydroid.cfg [properties] / waydroid_base.prop.";
    };
  };

  config = lib.mkIf cfg.enable {
    virtualisation.waydroid.package = wrappedWaydroid;
    systemd.services.fp6-waydroid-props = {
      description = "Apply FP6 Lomiri Waydroid display/suspend properties";
      wantedBy = [ "waydroid-container.service" ];
      before = [ "waydroid-container.service" ];
      unitConfig.ConditionPathExists = "/var/lib/waydroid/waydroid.cfg";
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${applyProps} ${propsJson}";
      };
    };
    system.build.fp6-waydroid-lomiri-props = applyProps;
    system.build.fp6-waydroid-lomiri-package = wrappedWaydroid;
    assertions = [{
      assertion = lib.mod cfg.density 20 == 0;
      message = "hardware.fairphone6.waydroidLomiri.density must be a multiple of 20 (GRID_UNIT_PX * 20).";
    }];
  };
}
