# Fairphone network policy

Rust data saver and smart Wi-Fi/mobile fallback for aarch64 Mobile NixOS with
Lomiri, Qt 5.15 and UI Toolkit 1.3. Includes the user daemon, privileged helper,
settings UI, CLI, NixOS module and reproducible local tests. No telemetry.

Data saver and automatic switching are both disabled by default. Foreground
apps are exempt, app background exceptions are configurable, and monthly usage
warnings/background limits are applied per app. Mobile routing requires the
coordinator's oFono adapter; foreground access requires the Lomiri focus hook.
These interfaces and operational limits are specified in [DESIGN.md](DESIGN.md).

## Build and test

```sh
export NIX_PATH=nixpkgs=channel:nixos-unstable
nix-shell
cargo test
cargo clippy --all-targets --features ui -- -D warnings
scripts/dbus-test.sh
scripts/netns-test.sh
scripts/netns-perm-test.sh
scripts/preview.sh
exit
nix-build
nix-instantiate --eval --strict --json tests/module.nix --arg system builtins.currentSystem
nix-instantiate --eval --strict --json tests/module.nix --argstr system aarch64-linux
```

`cargo test` needs only stable Rust, not Qt. The `ui` feature requires Qt 5 qmake
and development libraries. `Cargo.lock` is included. The development shell supplies
Rust, Qt, Lomiri UITK, test tools and QML import paths. If HOME/cache is read-only,
use `XDG_CACHE_HOME=/tmp/netpolicy-cache CARGO_HOME="$PWD/.cargo"` for the commands.
The Nix derivation uses `rustPlatform.buildRustPackage` and runs the Rust tests.
`default.nix` supplies a Qt 5 Lomiri toolkit override because current nixpkgs also
ships Qt 6 Lomiri packages. `package.nix` accepts `lomiriUiToolkit` for downstream
integration with an existing Qt 5 stack.

The D-Bus test uses a new private bus and mocks NetworkManager; it also checks the
real helper's caller-UID authorization with a fake nft command. These two tests
are marked ignored in ordinary `cargo test` and explicitly run by the script.
The namespace integration creates local veth links and a synthetic HTTP server,
checks IPv4/IPv6 allow/block/restore and RX/TX counter persistence, and never changes
the host firewall. It runs without root when user/network namespaces and nft socket
matching are available. Exit 77 means unsupported; use the same script as root in
a systemd scope on a disposable test system if necessary (keep the build toolchain
in PATH). It cleans up its processes and namespaces automatically. The root-only
phone checklist below covers real per-app scopes and existing connections.

Previews use `QT_QPA_PLATFORM=offscreen`, Mesa software OpenGL and Xvfb. UITK's
LomiriShape requires OpenGL, so **do not** use `QT_QUICK_BACKEND=software`.
`previews/settings.png`, `usage.png`, and `thresholds.png` are actual renders.
Preview mode makes no D-Bus calls and shows no invented app usage. Run the normal
UI with `QT_QPA_PLATFORM=wayland netpolicy-ui` inside Lomiri; desktop X11 can use
`xcb`. The development shell defaults offscreen only for reproducible previews.

## NixOS installation

```nix
{
  imports = [ ./netpolicy/module.nix ];
  hardware.fairphone6.netpolicy = {
    enable = true;
    user = "phablet"; # an existing user
    uid = 1000;
    apps = [
      { id = "org.example.Chat"; unit_pattern = "app-org.example.Chat-*.scope"; }
      { id = "morph-browser"; unit_pattern = "lomiri-app-launch-morph-browser-*.scope"; }
    ];
    # Supply the coordinator adapter described in DESIGN.md:
    # mobileAdapter = "${myOfonoAdapter}/bin/netpolicy-mobile";
    # Add verified service basenames for the phone's VPN/push implementation:
    # systemAllow = [ "NetworkManager.service" "systemd-resolved.service"
    #                 "systemd-timesyncd.service" "my-vpn.service" "my-push.service" ];
  };
}
```

The app IDs and unit patterns above are **illustrative configuration**, not claims
about installed phone apps. Inspect `systemctl --user list-units` and each app's
`ControlGroup`, then configure accurate globs. Do not map a common parent scope to
multiple app IDs. App processes must enter their cgroup before creating sockets.
The default empty map does not guess app identity: saver blocks unmatched user
background traffic on costly links, and the usage page explains the empty list.

The module installs `fp6-netpolicy-helper.service` (system),
`fp6-netpolicy.service` (user, graphical-session target), restrictive D-Bus policy,
`nft_socket`/`nf_tables` module requests, and a desktop entry with
`X-Lomiri-Touch=true`. The user must match the configured UID. Supply a kernel with
cgroup v2, `CONFIG_SOCK_CGROUP_DATA`, `CONFIG_NF_TABLES`, `CONFIG_NFT_SOCKET` and
IPv4/IPv6 socket lookup support. Keep existing firewall rules; only the
`fp6_netpolicy` table belongs to this package.

Per-app network permission (app-permissions module): with
`permissionsFile = "/run/fp6-permissions/network.json"` the helper also drops
the traffic of apps the user denied Wi-Fi, mobile data, VPN or the local
network in Settings > Privacy > App permissions, on every interface and
independent of data saver ([DESIGN.md](DESIGN.md) "App network permission";
`scripts/netns-perm-test.sh` tests it in a network namespace).

The UI exposes saver, allow-list, per-app metered/mobile usage, smart switching,
capped-plan consent, warning/background limits, bad/good sample counts, cooldown,
loss, latency, signal threshold and optional connectivity URL. Settings are local
JSON; there are no GSettings `ai`/`ax` types. The UITK layout uses margins rather
than unsupported Label top/bottom padding.

## Manual checklist for the real phone

No phone was accessed during implementation. After coordinator integration:

- Confirm the services start, the desktop launcher is touch-enabled, all three
  pages render on Qt 5.15, and Maliit appears for numeric/URL fields. Confirm Save,
  back navigation, error display and restored settings after a daemon restart.
- Verify actual `ControlGroup` paths for two launched apps, including a restart
  and multiple instances. Launch test transfers **after** entering their scopes.
- On unmetered Wi-Fi, both apps should work. On metered Wi-Fi and mobile, the
  foreground app should work; background should pause unless allow-listed.
  Existing TCP connections must also stop sending after policy changes. Test
  IPv4 and IPv6. Move focus, lock the screen, and stop focus renewals to test expiry.
- Check DNS, DHCP renewals, NTP, explicit push/VPN exceptions and restricted
  updates. Check each VPN's actual tunnel and transport scopes for attribution
  and leakage. Tethered traffic is not covered. Validate offline/local-network
  behavior before deploying a restrictive system exception list.
- Lower warning/limit settings, transfer known test data and observe notification,
  counters and background blocking. Foreground remains allowed at the limit.
  Restart apps and helper, simulate a reboot/month boundary in a disposable test
  environment, and confirm no double counting. Expect estimated, sampled totals.
- Enable smart switching on an explicitly permitted plan. Degrade the gateway or
  block the configured endpoint; verify consecutive bad samples, the adapter's
  usable mobile default route, explanatory notification and continued Wi-Fi
  association. Restore Wi-Fi and wait for good samples plus cooldown. Check
  captive portals, IPv6-only Wi-Fi and a gateway that suppresses ICMP.
- Verify a capped plan without consent never activates mobile, even under repeated
  failures. Revoke consent/disable switching while fallback is active; verify
  restoration. Test unavailable SIM, airplane mode, roaming restrictions, adapter
  error/timeout and manually enabled mobile data without stealing ownership.
- Stop/crash the user daemon and helper. Verify foreground leases expire, owned
  routes recover through the adapter, nft errors are visible, and restart restores
  enforcement. Confirm unrelated firewall tables and other users are untouched.
- Wire the quick-settings tile to `SetDataSaver`, open settings on long press,
  and renew `SetForeground` from Lomiri as described in DESIGN.md.

## Patch history

The source checkout's `.git` was read-only. Numbered mail patches in `patches/`
preserve the implementation steps against the initial task commit. Apply them to
a **clean** clone at that base with `git am patches/*.patch`; do not apply them
on top of the already modified working tree. See `patches/series` and
[FINAL_REPORT.md](FINAL_REPORT.md) for validation evidence and limitations.
