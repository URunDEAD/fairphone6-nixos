# Design and security

## Scope and reuse

The application is three components: `quickshare-core` owns LAN transport and
receive policy, `fp6-quickshare-daemon` owns the session D-Bus API and consent,
and a small Qt 5 C++ bridge connects the Lomiri UI Toolkit 1.3 QML application
to that API. The CLI uses the same API. No Tauri/WebKit runtime is needed.

The actual Nearby/Quick Share wire protocol comes from GPL-3.0 `rqs_lib`
0.11.5, pinned at `378d8ae969941bee4bf60ad34ac9cf8bb7005eb7`. Its complete
protobuf definitions, P-256 UKEY2 exchange, PIN derivation and encrypted
payload framing are reused. The fork is suitable for Wi-Fi LAN; implementing
this protocol afresh would add avoidable interoperability and cryptographic
risk. Its original receive policy was unsuitable for this task, so the source
is vendored for reviewable security fixes. `vendor/UPSTREAM.md` records sources
and licenses. NearDrop's protocol notes were studied; no Swift code is copied.

The Apache-2.0/MIT `mdns-sd` fork is also pinned and vendored. All other Rust
sources are crates.io dependencies pinned by `Cargo.lock`. Runtime code has no
telemetry, Google account access, HTTP client, or internet rendezvous service.
Nix's ordinary dependency fetches are needed to build the package.

Local rqs changes:

- The Tauri-facing RQS lifecycle, BLE modules and TypeScript generation are
  excluded from the compiled library. The daemon owns discovery, visibility,
  names, interface restrictions and lifetimes; the old source files remain
  as upstream reference. Optional BLE/system-metrics dependencies are removed.
- A per-connection receive directory replaces the upstream process-global
  download directory. This also allows two real protocol instances in tests.
- Directory-relative, exclusive file creation; filename/size/free-space checks;
  consent gating; completion-length verification; cleanup of incomplete files.
- Constant-time HMAC verification, bounded payload assembly, strict AES header
  checks, invalid P-256 points returning errors instead of panicking, left
  padding of short coordinates, and cancellation-safe length-prefix reads.
- Text/URL sending and zero-byte file sending, which upstream did not support.

Rust crates forbid unsafe code. Third-party crates implement OS interfaces and
cryptographic primitives; this is not a claim that every dependency is unsafe-free
or that the application has had an independent security audit.

## Protocol

Receivers advertise `_FC9F5ED42C8A._tcp.local.` over IPv4 mDNS. The instance
contains a random four-character endpoint identifier; TXT `n` contains the
encoded device type, random identification bytes, and device name. The daemon
uses TCP **53317**, configurable as one fixed port, so no ephemeral inbound
port range is opened. Outbound TCP uses an ordinary ephemeral source port.

A connection starts with protobuf Nearby Connections frames with a four-byte
big-endian length. UKEY2 negotiates P256-SHA512 and
`AES_256_CBC-HMAC_SHA256`, validates the initiator's commitment, performs ECDH,
and derives authentication and traffic secrets with HKDF-SHA256. The
transcript-derived four-digit PIN must match on both screens. Encrypted
SecureMessage frames carry monotonic sequence numbers, AES-CBC ciphertext,
and an HMAC-SHA256 authentication tag. Paired-key negotiation returns Unable:
there is no Google account identity or contacts authentication. An introduction
lists attachments; only a later explicit Accept response authorizes payloads.

Protocol references:

- [rquickshare sources and Android discovery FAQ](https://github.com/Martichou/rquickshare/tree/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7)
- [NearDrop's LAN protocol and BLE notes](https://github.com/grishka/NearDrop/blob/add655c6c5a40b9ba03c0329b598f2817185f5a9/PROTOCOL.md)
- [Google's UKEY2 implementation and protocol specification](https://github.com/google/ukey2)

## Android discovery and visibility

Stock Android can discover our mDNS receiver when both devices are on a LAN
that permits multicast and peer traffic. That does **not** mean every Android
version continuously advertises itself as a receiver when Everyone or Contacts
is selected. rquickshare and NearDrop document that Android often waits for a
specific BLE advertisement before publishing mDNS. Opening Android's Quick
Share **Receive** screen may make it visible without BLE; this varies by
Google Play services version and vendor, especially Samsung. Test the stock
receive screen first; some versions may need a supported QR/BLE workflow.
No companion APK is required by the LAN protocol, but universal stock-phone
discovery is not established by our loopback tests.

BLE advertising/listening is not enabled in this build. The Fairphone itself
advertises mDNS for the whole explicit visibility window; it does not depend
on BLE wake-up. Discovery and receiving are independent: looking for targets
does not make this receiver visible. The app offers a ten-minute visibility
switch. The daemon starts OFF every time, including after a crash/restart;
`everyone-for-10-min` expires after 600 monotonic seconds (the event loop checks
every second). `everyone` requires an explicit D-Bus/CLI request. `contacts`
is rejected, rather than implying authentication that does not exist.

## Consent and filesystem policy

Only the user session bus exposes the service. Other processes with access to
that user's bus are trusted as the user; there is no system-bus API or elevated
service. A same-user process can approve a request. Remote peers cannot.
Incoming requests have a 60-second consent deadline. The daemon validates the
transfer ID and pending state for Accept/Reject. Visibility off rejects pending
requests; already accepted transfers may complete. Failed and stale requests
cannot be accepted again. An explicit `receive --accept-for-testing` CLI flag
is the only unattended consent bypass, and it is not used by the app/service.
PIN comparison is a human responsibility; names and PIN displays are not a
contact identity proof. Never accept an unexpected transfer or mismatched PIN.

Files go to the literal `~/Downloads/Quick Share/` by default. Each path
component is opened with NOFOLLOW and DIRECTORY using safe rustix APIs;
symlinked Downloads directories are deliberately rejected. The final directory
is 0700. Files are created 0600 with CREATE|EXCL|NOFOLLOW, using a held directory
file descriptor. Name collisions produce `name (1).ext`, `name (2).ext`, etc.,
including collisions with dangling symlinks. Nothing truncates an existing
file. Absolute paths, separators (including backslashes), `.`/`..`, NUL, and
empty names are rejected. Control characters and unsafe punctuation are
replaced; UTF-8 filenames are bounded to leave suffix room. The new local names
are reflected in transfer snapshots after acceptance.

The default received total limit is 1 GiB per transfer and 128 files. Negative
sizes, duplicate payload IDs, overflowing totals, unexpected offsets, excess
bytes and short final chunks are rejected. Available space is checked at
introduction and again on acceptance, retaining a 16 MiB reserve. This is not
a disk reservation: concurrent writes or a filling disk can still fail a
transfer. File data is synced before success. Error, reject and cancellation
drop incomplete transfers and unlink their newly created files. Abrupt process
termination or power loss can leave partial files; no completion should be
inferred from their presence alone. An external rename by a process with the
same user's access is outside the threat model.

Text and URLs require the same consent and are limited to 64 KiB. They remain
in bounded in-memory history, are shown in a notification, and can be copied
or opened only after a user action. Notification content is escaped; the GUI
uses plain text for incoming filenames and sender labels. Only HTTP(S) links
can be opened as URLs; received links never execute shell commands. Completed
files open their containing directory. The daemon does not automatically open
or execute any received attachment. Clipboard copy requires the GUI session.

Eight inbound sessions are allowed at once; frame size is capped at 5 MiB,
byte-payload buffers/counts are capped, and inactive protocol operations time
out after 120 seconds. History is limited to 128 transfers and is not persisted.
Local-LAN denial of service and unsolicited requests remain possible while
visible. Do not expose the TCP listener to an untrusted routed network.

## D-Bus contract

Session bus name/interface: `com.fairphone6.QuickShare1`.
Object: `/com/fairphone6/QuickShare1`.

| Member | Signature / meaning |
| --- | --- |
| Visibility | read/write `s`: off, everyone-for-10-min, everyone |
| DeviceName | read/write `s`, 1–64 UTF-8 bytes, no controls |
| State | read `s`: idle, discovering, transferring |
| StartDiscovery / StopDiscovery | no arguments |
| Send | `s target, as items → s transfer_id` |
| Accept / Reject / Cancel | `s transfer_id` |
| DeviceFound / DeviceLost | `ss id,name` / `s id` |
| IncomingRequest | `s id, s sender, as filenames, s pin` |
| Progress | `s id, t bytes, t total` |
| Finished | `s id, s status` (finished, failed, rejected, cancelled) |
| ListDevices / ListTransfers | no arguments → JSON string |
| Snapshot | no arguments → JSON string (UI state and both lists) |

Targets are IDs returned by discovery, not arbitrary host strings. `Send`
accepts local `file://` URIs, or exactly one `text:literal` or `url:https://…`
item. Mixing text with file attachments is unsupported. Unknown/stale IDs and
unsupported URI schemes produce D-Bus errors. Additional JSON snapshot methods
make reconnecting and missed signals recoverable without Qt 5 custom metatypes.
Signals and PropertiesChanged are emitted as state changes; the GUI also
refreshes once a second. Sender completion currently means the local encrypted
send finished, not a separately verified remote disk-durability acknowledgement.

## NixOS and network boundaries

The module installs the desktop entry, content-hub share peer, D-Bus activation
file, CLI/app and a systemd **user** service associated with the graphical
session. No root daemon is used. The service has UMask 0077 and starts OFF. It shares the session’s `/tmp`
namespace so file:// URIs from other apps and the CLI remain accessible.
The module opens TCP 53317 (or the configured single port) and UDP 5353 only
under explicit `networking.firewall.interfaces.<wifiInterface>` rules. It does
not add global TCP/UDP rules. Interface names must start with `wl`; an explicit
allowlist is required. Existing system-wide rules from other modules remain
the administrator's responsibility.

mDNS announcement/browsing is embedded in the transport and restricted to the
same interface allowlist, including address validation for incoming TCP and
outgoing on-subnet targets. The firewall enforces the ingress interface
boundary. Avahi is enabled for system `.local` name resolution and coexists
with the embedded mDNS implementation; `openFirewall = false` and
`allowInterfaces` prevent Avahi from opening a global rule. Network multicast
filtering, AP client isolation, VPN routing and suspend/resume require device
validation.

## Known gaps

No BLE wake-up, Google contacts/account certificates, QR rendezvous, Wi-Fi
Direct/hotspot upgrades, IPv6 transport, WebRTC/relay, background clipboard
service or Windows/macOS build. Android behavior varies and has not been
validated on physical hardware here. Content-hub registration, notification
actions, clipboard ownership and mobile suspend/resume need a real Lomiri
session. There is no automatic persistent partial-file cleanup after SIGKILL.
The upstream protocol implementation is not formally verified; tests establish
local behavior, not complete Android compatibility. aarch64 cross-building and
Fairphone hardware validation are coordinator tasks.
