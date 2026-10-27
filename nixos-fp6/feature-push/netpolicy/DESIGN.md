# Network policy design and coordinator contract

## Processes and trust boundaries

`netpolicyd` is a systemd **user** service on the session bus. It owns settings,
foreground leases, hysteresis and notifications. `netpolicy-helper` runs as root
on the system bus; it alone discovers cgroups, applies nftables, samples counters,
performs interface-bound probes and invokes the mobile adapter. `netpolicy-ui`
is a Rust Qt 5 bridge hosting Lomiri UI Toolkit 1.3 QML. `netpolicyctl` uses the
same session API, including the quick-settings operation.

The helper checks the unique sender's UID using `org.freedesktop.DBus` on every
method. The Nix module also installs a deny-by-default system bus policy. Only
one configured UID is supported. Session applications with that UID are trusted:
this is a user preference service, not a sandbox against hostile same-user code.
There is no arbitrary shell, nft rule, path or cgroup argument in the public API.
App mappings, system exceptions and the adapter executable are root configuration.
The helper validates app IDs, paths, interface names and settings before use.
Commands use fixed argument vectors and a 20-second timeout; D-Bus calls have
5-second (UI) or 25-second (system) timeouts. The Rust library forbids unsafe code.
The UI has one documented unsafe QObject pin, required by qmetaobject; the bridge
outlives its QML engine and is never moved while Qt holds a reference.

## NetworkManager integration

The helper polls these documented D-Bus APIs; it does **not** change
`WirelessEnabled`, `WwanEnabled`, disconnect Wi-Fi or modify saved connections:

| Object/interface | Properties / methods used |
| --- | --- |
| `org.freedesktop.NetworkManager` | `GetDevices`, `ConnectivityCheckUri`, `ConnectivityCheckEnabled` |
| `…Device` | `State`, `DeviceType`, `IpInterface`, `Metered`, `Ip4Config`, `Ip6Config`, `Ip4Connectivity`, `Ip6Connectivity` |
| `…Device.Wireless` | `ActiveAccessPoint` |
| `…AccessPoint` | `Strength` (0–100) |
| `…IP4Config`, `…IP6Config` | `Gateway` |

Only activated devices (state 100) are considered. Wi-Fi is device type 2 and
NM modems are type 8. Metered YES / GUESS_YES and UNKNOWN are chargeable; NO and
GUESS_NO are not. Interfaces, not NM's global connectivity result, decide which
traffic is restricted. Connectivity is read on the Wi-Fi device so mobile cannot
mask a captive Wi-Fi link. The last successfully enumerated Wi-Fi device is used
if several are active (the phone is expected to have one).

Mobile oFono data is **separate** from NM's modem device and WWAN radio switch.
Its interface and activity come from the adapter below. No real oFono integration
is guessed. TUN, IP tunnel and WireGuard devices are included conservatively when
any chargeable/available mobile uplink exists, so original sockets can be matched
before VPN encapsulation. With simultaneous uplinks, this can also restrict a VPN
using unmetered Wi-Fi. More exact per-tunnel underlay attribution is future work.

References consulted:

- [NetworkManager manager API](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html)
- [NetworkManager Device API](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Device.html)
- [NetworkManager Wireless API](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.Device.Wireless.html)
- [nftables manual: socket expressions and counters](https://netfilter.org/projects/nftables/manpage.html)

## Mobile adapter: the coordinator's implementation point

Set `hardware.fairphone6.netpolicy.mobileAdapter` to an absolute, root-owned,
non-user-writable executable (prefer a Nix store path). The helper executes it
without a shell, with exactly one of these arguments:

| Argument | Required behavior |
| --- | --- |
| `status` | Report availability, active data context, stable/reserved interface name and whether this policy owns a route override. No mutations. |
| `prefer-mobile` | Save the prior data/routing state, enable the oFono context if needed, verify it is usable, then prefer its IPv4/IPv6 default routes. Keep Wi-Fi associated and usable for interface-bound probes. Return only after success. |
| `prefer-wifi` | Restore **only** changes owned by this policy. Preserve a mobile context already enabled by the user, tethering or another service. Idempotent. |

Every successful command writes one JSON object to stdout:

```json
{"available":true,"active":false,"owned":false,"interface":"wwan0"}
```

`available` must also account for airplane mode, SIM state, roaming consent,
carrier restrictions and the ability to perform routing safely. When `available`
or `active` is true, `interface` is mandatory and must be stable across activation.
The helper installs rules on that interface **before** `prefer-mobile`. The
adapter must not rename it or route through an unprotected interface. If the
platform allocates interfaces dynamically, reserve/discover the interface before
advertising `available`; do not advertise availability until this can be guaranteed.

The adapter must return `active=true, owned=true` after `prefer-mobile`, and
`owned=false` after restoration. Persist ownership and the previous route/context
state root-only so helper restarts can recover. A partial failure must roll back;
return nonzero with a short diagnostic on stderr. An absent adapter reports
unavailable and never enables mobile. Its maximum command duration is 20 seconds;
keep `status` fast. Use D-Bus/system networking interfaces inside the adapter, not
an unbounded background process. The module gives the helper NET_ADMIN/NET_RAW
and a writable state directory, but not general writable system configuration.

A helper-side 30-second coordinator lease restores an owned override after daemon
loss. Disabling switching or revoking capped-plan consent also restores it. Normal
service stop invokes adapter restoration before deleting the helper's own table;
on restoration failure the table is retained. Hardware loss or SIGKILL cannot
guarantee immediate restoration; the adapter must support idempotent recovery.

## Session API, foreground hook and quick-settings tile

Bus name/interface `org.fairphone.NetPolicy1`, object `/org/fairphone/NetPolicy1`:

| Method | In → Out | Contract |
| --- | --- | --- |
| `GetStatus` | `()` → `s` | JSON: settings, helper snapshot, foreground, routing message, link metrics |
| `SetSettings` | `s` → `()` | Validated JSON settings, atomically persisted |
| `SetDataSaver` | `b` → `()` | Narrow tile operation; preserves other settings |
| `SetForeground` | `s` → `()` | Configured app id, or empty string for no foreground |

Lomiri must translate focus changes to the **same app id** configured in `apps`,
call `SetForeground`, and renew it every five seconds while that app is focused.
Send empty on lock screen, focus loss or shell shutdown. The user daemon expires
the focus lease after 15 seconds; the helper has an independent 15-second lease.
Data saver never blocks the current, valid foreground app, including at its
background quota. Without the shell hook, apps are treated as background.

The coordinator's quick-settings tile should read `GetStatus.settings.data_saver`,
call `SetDataSaver(bool)` on tap, and open `org.fairphone.netpolicy.desktop` on
long press. Refresh the state after a successful call and when the tile opens;
poll while visible (there is no custom change signal). Display `status.message`
on failure rather than optimistic state. Examples:

```sh
netpolicyctl saver on
netpolicyctl foreground org.example.Chat
netpolicyctl status
```

The system API is `org.fairphone.NetPolicy.Helper1` at
`/org/fairphone/NetPolicy/Helper1`: `Sync(s settings, s foreground) → s snapshot`
and `SetMobilePreferred(b) → ()`. The snapshot includes app IDs, UTC monthly
usage, NM network state, adapter state, fresh sample ID and any helper error.
Repeated reads of one sample never advance hysteresis.

## App matching, system services and nftables

Applications must **create sockets after entering their user service/scope**.
The helper recursively finds configured unit basename globs under
`user.slice/user-UID.slice/user@UID.service`, including `app.slice` and nested
Lomiri launch slices. Discovery is bounded to eight levels and 1024 scopes.
Ambiguous mappings are rejected. Systemd's escaped unit names are supported.
Multiple instances/scopes of an app share named counters. Cgroup inode changes
force a rule refresh; named paths alone are insufficient because nft resolves
cgroup IDs at rule installation.

The table is `inet fp6_netpolicy`, with input/output filter chains at priority
-5. There is no global ruleset flush, no established-connection bypass, and no
packet content logging. Atomic chain replacement preserves named TX/RX counters.
Deepest app scopes win, preventing duplicate accounting. Counter names hex-encode
app IDs without collisions. Loopback is exempt; existing firewalls can still drop
traffic that this service permits.

On a costly interface, policy order is:

1. Explicit system service exceptions and the helper's probe service.
2. Known app: allow valid foreground; otherwise enforce its per-app monthly
   background limit; then allow if saver is off or the app is allow-listed.
3. If saver is on, drop unmatched sockets under the configured user's slice and
   `system.slice`; output also has a UID fallback for unattributed user sockets.
4. Other users and traffic without a match retain the existing firewall policy.

The module's default system exceptions are NetworkManager (including DHCP/DNS
where it provides them), systemd-resolved, and systemd-timesyncd. Add the actual
VPN and push service names to the explicit list. Updates are restricted by
default; add their units only if intentionally exempt. User-session push agents
can be mapped as apps and allow-listed. The configured system service basename
must be directly under `system.slice`; nested custom system slices require a
future path mapping extension. Root services outside `system.slice` are not
implicitly controlled. Exception processes, DNS proxies and VPN relays are trusted.

The enforcement scope is locally originated/received socket traffic. Forwarded
(tethered) traffic, kernel-only/no-socket packets, pre-existing sockets moved
between cgroups, shared proxies and VPN encapsulation can limit attribution.
Unknown apps are blocked when saver is on, but lack individual usage totals until
mapped. This is not a carrier billing meter or hostile-app containment boundary.

## Usage and thresholds

Counters sum matched permitted TX plus matched RX on chargeable interfaces.
Dropped TX does not consume the quota; matched RX is counted even when dropped,
since those bytes already traversed the radio. Retransmissions and IP headers
are included, link-layer overhead is not, and RX socket lookup can miss packets.
The UI therefore reports an estimate. Unmetered traffic is not counted.

The root-only `/var/lib/fp6-netpolicy/usage.json` stores UTC month, totals,
last raw counters and boot ID. Rule updates preserve counters; helper restarts
subtract the persisted baseline. Kernel counter decreases charge the new value;
boot/table recreation clears old baselines. UTC month rollover clears totals,
retains raw baselines and assigns the interval crossing midnight to the new month.
Historical months are not retained. Atomic rename, file and directory fsync protect
state writes. At most one unsaved polling interval can be lost at abrupt shutdown.
External resets of counters within the owned table are unsupported.

Settings use `$XDG_CONFIG_HOME/fp6-netpolicy/settings.json` (fallback
`~/.config`). Warning and limit thresholds are common settings applied **per app**;
0 disables each threshold. Allow-list bypasses saver but does not bypass a reached
background quota. Foreground always wins. Warning notifications occur once per app
per month in a daemon session; restarting the daemon can repeat a warning. Quotas
are sampled, not byte-exact: a transfer can exceed a limit within one polling
interval, and foreground traffic can exceed it without restriction.

## Quality monitoring and hysteresis

No probes run unless smart switching is enabled. The helper sends three bounded
ICMP gateway probes and one HTTP(S) request bound to the Wi-Fi interface. The URL
is the explicit setting, or NM's `ConnectivityCheckUri` only when NM checks are
enabled. Blank/disabled NM configuration means no public check; no hardcoded third
party fallback is substituted. System DNS resolution still follows system resolver
policy and may use another uplink. No cookies, credentials, proxies or redirects
are used; response size is capped at 4096 bytes and response contents are discarded.
Valid online responses are 204, or 200 with the default NetworkManager online body
or `X-NetworkManager-Status: online`. Custom NM response bodies need an endpoint
using one of those forms. HTTP endpoints cannot authenticate against interception;
configure HTTPS when available.

Bad samples include a captive portal, public check failure, high public latency,
or high gateway latency corroborated by loss/weak signal. Weak signal or missing
ICMP replies alone do not trigger fallback. Recovery requires successful public
checks, gateway response and loss/latency below half the bad thresholds. A router
that suppresses ICMP may prevent automatic recovery; disabling switching restores
the prior preference. Missing samples reset streaks. Default hysteresis: 3 bad,
5 good, 120-second minimum dwell. Capped plans default true; automatic switching
defaults false and requires explicit capped-plan permission. The state machine
commits a route change only after the adapter confirms it. Successful changes
produce explanatory desktop notifications.

The user/helper loops wait five seconds between iterations. Gateway/public probes
add up to eight seconds; IPC/adapter errors have separate timeouts. Focus changes,
new cgroups, link changes and limits take effect on the following reconciliation,
not instantly. The helper continues to expire focus and apply policy against the
last known interface set if NM or adapter observation fails. It reports errors
and suppresses new switches while observations are invalid. There is no guarantee
of strict enforcement while the helper itself is stopped; service stop removes
its owned table after restoring routes.

“Wi-Fi on automatically near saved networks” is explicitly out of scope: no
location collection, scan database, geofencing or automatic radio enabling.
