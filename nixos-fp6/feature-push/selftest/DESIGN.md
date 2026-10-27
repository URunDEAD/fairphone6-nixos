# Design

## Read-only boundary

The parent starts one subprocess per check, each in its own process group. It captures at most 2 MiB, discards stderr, enforces a configurable 1–30 second deadline (default 5), kills the entire group on timeout and reaps it. Base command subprocesses inherit the worker group. A stuck D-Bus call or filesystem operation cannot block subsequent checks. Workers return authored summaries, enum statuses and counts. No raw system output, D-Bus reply, XML, filename, device address, SSID, account identity, SIM identifier, clipboard, call history, journal message or exception is printed or persisted.

Connection addresses are restricted to local Unix D-Bus transports. Names are queried with NameHasOwner/GetNameOwner and ListActivatableNames. All feature calls, including introspection and property reads, target the **unique owner**, preventing implicit activation even when an owner disappears between calls. No service starts, radio controls, discovery, audio, networking or sensor requests are made. Verified sibling entries also carry their installed systemd unit name: a missing bus name on an active/failed installed unit is FAIL, and an inactive installed unit is WARN. An advertised but stopped service is WARN; an absent optional feature is SKIP; absent `required=true` is FAIL. Being activatable is not enough to claim health.

The feature manifest is trusted administrator configuration. Its zero-argument methods have a narrow source-audited allowlist; properties use only Get. No arbitrary commands or argument bodies can be configured. A new method requires a code review as well as a manifest update. A method name alone cannot guarantee purity on an arbitrary third-party service; extension authors must audit the actual implementation. Introspection must contain the named interface and readable property or no-input method. D-Bus bodies and JSON fields are type checked. Enum values can be constrained. Sensitive reply contents exist transiently in worker memory and are discarded.

A shared redaction pass further masks familiar identifier forms in every check summary. This is defense in depth, not the privacy boundary: raw data never becomes a report string. Unknown journal unit names are grouped under `other`, because templated unit names can contain identifiers. Known units retain fixed category labels. The bugreport sibling's redaction/section-reduction approach informed this design; no dependency or writable link to that repository is required.

The report directory is created as 0700 and checked for ownership, permissions and symlink components. Temporary files and atomic no-clobber publication use 0600. Existing paths are never overwritten. Each file is individually atomic; a disk failure between publishing JSON and text can leave one complete file. Failure is then reflected on stdout as FAIL. Same-user hostile directory races are not a privilege boundary; there is no privileged report writer. No automatic deletion/retention policy runs. `--no-save` performs no intentional persistent writes.

## Actual feature contracts inspected

| Sibling source | Endpoint / probe | Validation |
|---|---|---|
| `quick-share/crates/fp6-quickshare/src/bin/daemon.rs` | session `com.fairphone6.QuickShare1`, `/com/fairphone6/QuickShare1`, State property | string: idle/discovering/transferring |
| `webshare/src/control.rs`, `data/com.fairphone6.WebShare1.xml`, `src/session.rs` | session WebShare1, same path naming, Status | JSON object, active boolean, files/pending arrays |
| `kdeconnect/src/dbus.rs`, `src/app.rs` | session KdeConnect1, Devices | JSON object, devices/plugins arrays; identities discarded |
| `ocr/src/service.rs` | session Ocr1, GetSettings | JSON object, index_enabled boolean |
| `dictation/docs/com.fairphone6.Dictation1.xml` | session Dictation1, State property | nonempty bounded string |
| `sharesheet/src/service.rs` | session Share1, Snapshot | JSON object, revision number, busy boolean, targets array |
| `callkit/src/service.rs`, `docs/DBUS_API.md` | session Calls1, Calls | JSON array of caller-owned calls; no shell permission requested |
| `netpolicy/src/daemon.rs` | session `org.fairphone.NetPolicy1`, `/org/fairphone/NetPolicy1`, GetStatus | JSON object, settings/snapshot objects and route string |
| `netpolicy/src/helper.rs` | system `org.fairphone.NetPolicy.Helper1`, `/org/fairphone/NetPolicy/Helper1` | introspection only; WARN because Sync and SetMobilePreferred change state |
| standard notifications | session `org.freedesktop.Notifications`, GetServerInformation | four strings |

Interfaces match bus names for these entries. KDE Connect ClipboardState is deliberately excluded: it calls `clear_clipboard_if_unused`, so it is not strictly read-only. Share Snapshot only clones stored state; it does not refresh discovery. Calls Snapshot requires shell ownership; Calls is the safe provider-level read.

UsbModes1, Update1, DND, battery, app-ready, privacy and gestures are explicit **unverified integration slots**. Referenced sibling sources do not establish their real phone contracts. The callkit Notifications1 example is a proposal/mock, not evidence of an installed daemon. Slots inspect a candidate endpoint when owned but never call a guessed status method. Set their actual bus, path and interface; set `unverified=false`, an audited property/method and reply shape before accepting them as health coverage. Do not interpret absent candidates as proof the phone lacks that feature. A coordinator-supplied manifest replaces the list (copy and edit the shipped file).

## Base checks and limits

- `/etc/fp6-release` must be readable/nonempty; its contents are not printed.
- System/user failed-unit lists return counts. Display manager and `lomiri.service` must be active. Maliit is checked by exact process name, not launched.
- Journal checks read only the current boot, a maximum of 1000 records and explicit unit/priority fields. First check journal visibility; empty/inaccessible scope is WARN. Coredumps are counts of systemd-coredump journal records, not core files or unique crash IDs. Permission-limited views cannot certify the entire machine.
- Root/home usage: WARN ≥85%, FAIL ≥95%. Memory availability: WARN <10%, FAIL <5%. Active zram swap is inspected in `/proc/swaps`.
- `wpctl inspect` resolves existing default sink/source without playback, recording or routing. It uses the local PipeWire connection. NetworkManager State is read without any connectivity request. Bluetooth GetManagedObjects checks Adapter1, never starts discovery.
- oFono checks every exposed modem for Powered/Online, SIM Present and NetworkRegistration Status registered/roaming. ConnectionManager Powered is treated as mobile-data enabled; if enabled, an active internet context must exist. Disabled data is SKIP, not a request to activate it. Offline, absent SIM and unregistered states are WARN because user intent may explain them.
- GeoClue presence does not request a location. fprint GetDevices checks reachability/hardware only, never claims, enrolls or verifies a finger. Other phone GPS/fingerprint stacks require an adapter/configuration extension.
- Schema roots include XDG data locations, GSETTINGS_SCHEMA_DIR and Nix `share/gsettings-schemas/*/glib-2.0/schemas`. Source XML, including inherited keys across schema files, is inspected and containing directories strictly dry-run compiled. Compiled-only installations lacking source XML are not certified; no schemas found is SKIP.
- Desktop roots include XDG applications, icons and pixmaps. All X-Lomiri-Touch entries found are checked, including masked entries (conservative audit). Exec checks existence and executable bits, optional TryExec, simple env wrappers and absolute/themed icons. It never runs Exec, shell script bodies or tests launch success. Icon existence across installed themes does not guarantee a particular theme can resolve an inherited icon. Traversal is bounded and incomplete scans WARN.

## OTA and systemd integration

The module is opt-in under `hardware.fairphone6.selftest.enable`. Both oneshots run unprivileged as/for the phone user, accept only AF_UNIX sockets, use 0077 umask and a read-only system filesystem. Reports remain accessible to that user; journal privileges are not silently enlarged. System-only quick checks run two minutes after boot; full session checks run after graphical session activation. This covers OTA reboots without writing an update marker. Invocations are independent; no feature settings or services are changed.

The updater's **local** `fp6-updated start check` can invoke:

```text
fp6-selftest --quick --json
```

Execute in the phone user's established session (HOME, XDG_RUNTIME_DIR, DBUS_SESSION_BUS_ADDRESS), not the root updater's environment. Parse format_version=1 and the per-check array. Exit 0 means no WARN/FAIL among executed checks; it does not mean optional/skipped features were tested. Exit 1 means warnings/incomplete coverage, exit 2 means failure. Apply an OTA-specific allowlist of mandatory checks. A service with an unresolved probe is never sufficient evidence for marking a slot good. Save the redacted JSON or reference its private report; do not consume stderr or raw logs. Do not make this hook start/stop other services or roll back autonomously.

Before a session exists, the updater may run `--system-only --quick --json` as the target user, then defer session acceptance until the user report. The supplied module does not patch the updater, touch shared trial files, install remote hooks or change slot acceptance policy. These changes belong to the coordinator.

## UI and tests

The Rust Qt bridge launches the CLI on a background thread and sends parsed JSON back to QML by polling a channel. UI strings use plain text. The UI shares the CLI's summaries, serialization and report writer. UITK 1.3 labels use margins rather than unsupported padding properties. A Settings ItemPage launches the URI `fp6-selftest:run`; only this exact URI requests an immediate run. Other URIs are ignored. Preview mode never runs the CLI and renders authored synthetic results using QT_QPA_PLATFORM=offscreen.

Private-bus tests cover service absence, activatable-but-stopped behavior, introspection, property/method success, malformed reply shapes and deadlines. Filesystem fixtures cover strict schema compilation, forbidden types, executable/icon checks, redaction and private output. A process test verifies descendants cannot outlive a deadline. No test accesses a phone, remote host, the real user bus or personal data. There is no handwritten unsafe Rust; qmetaobject's generated Qt FFI remains in the optional UI dependency.
