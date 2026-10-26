# NixOS mobile boot & shutdown animation

## Contents

| Path | What | Use it for |
|---|---|---|
| `plymouth-theme-nixos-mobile/` | Ready Plymouth theme (script module) + Nix package | NixOS boot splash, the main target |
| `frames/boot/`, `frames/shutdown/` | Full-screen PNG masters, 1080×2340, 30 fps, 210 frames each (7 s) | Re-encoding, Android `bootanimation.zip`, other splash tools |
| `preview/*.mp4` | H.264 previews of the masters | Reviewing, sharing |

Background colour: `#05070D`. The logo occupies the middle 74.4% of the screen width (including its glow).

## Plymouth theme behaviour

- **Boot:** plays the intro (logo spins in and settles, 2.6 s), then loops a 2.4 s glow-and-sheen segment until boot finishes.
- **Shutdown / reboot:** plays the logo outro, then draws the flash and CRT line-to-dot collapse in the script.
- Sprites are 464×446 RGBA at 25 fps, scaled to the screen width. Only the current mode's frames are loaded (about 100 MB of decoded images).

## NixOS configuration

Copy `plymouth-theme-nixos-mobile/` next to your configuration, then:

```nix
{ pkgs, ... }:
{
  boot.plymouth = {
    enable = true;
    theme = "nixos-mobile";
    themePackages = [ (pkgs.callPackage ./plymouth-theme-nixos-mobile { }) ];
  };
  boot.initrd.systemd.enable = true;   # Plymouth starts earlier and handles shutdown cleanly
  boot.consoleLogLevel = 0;
  boot.initrd.verbose = false;
  boot.kernelParams = [ "quiet" "splash" "rd.udev.log_level=3" ];
}
```

Test without rebooting (as root): `plymouthd --debug; plymouth show-splash; sleep 8; plymouth quit`. For the shutdown animation: `plymouthd --mode=shutdown` instead.

## Notes

- There is no password prompt drawn by this theme. If you use LUKS, add `Plymouth.SetDisplayPasswordFunction` handling or keep a text fallback.
- To change size or frame rate, re-render from the design canvas and update `N_INTRO` / `N_LOOP` / `N_SHUT` and `FPS` at the top of the script.
- NixOS logo © NixOS contributors, CC-BY 4.0 (github.com/NixOS/nixos-artwork).
