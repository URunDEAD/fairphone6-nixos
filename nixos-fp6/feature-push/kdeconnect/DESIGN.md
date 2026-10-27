# Design and upstream study

The daemon is a Rust user service. QML is presentation; the session D-Bus API
`com.fairphone6.KdeConnect1` is the only control plane. LAN packets never invoke
shell commands. Trust is scoped to a device ID and an exact SHA-256 certificate
pin. Pairing always requires a local user action, expires in 30 seconds, and
shows the peer certificate fingerprint and the v8 verification code. Pairing
commands bind consent to that displayed fingerprint and verify it under the
same lock as the pending request, rejecting stale dialogs after reconnection.

## References inspected

Protocol behavior was studied from these upstream sources on 2026-10-27:

* [kdeconnect-kde](https://github.com/KDE/kdeconnect-kde), revision
  `ac8ceeb7b8110ddc255eec9d74e7e8d868004022`: `core/deviceinfo.h`,
  `core/backends/lan/lanlinkprovider.cpp`, `core/backends/pairinghandler.cpp`,
  and share/MPRIS plugins. GPL-2.0-only OR GPL-3.0-only OR KDE accepted GPL.
* [kdeconnect-android](https://github.com/KDE/kdeconnect-android), revision
  `b2a2b035b6d1d95116145301cb938133f5bb7ca7`: `backends/lan/LanLinkProvider.kt`.
  GPL-2.0-or-later. Confirms reversed main-channel TCP/TLS roles and v8
  encrypted identity exchange.
* [GSConnect LAN backend](https://github.com/GSConnect/gnome-shell-extension-gsconnect/blob/main/src/service/backends/lan.js):
  GPL-2.0-or-later; certificate-based trust and self-signed TLS handling.
* [Valent LAN channel](https://github.com/andyholmes/valent/blob/main/src/plugins/lan/valent-lan-channel.c):
  GPL-3.0-or-later; auxiliary transfers use conventional TCP/TLS roles, unlike
  the main channel.
* [kdeconnect-proto 0.2.1](https://docs.rs/kdeconnect-proto/0.2.1/kdeconnect_proto/):
  considered as a Rust transport abstraction. This implementation owns its small
  transport to keep Wi-Fi policy, certificate pinning, bounded parsing and user
  consent under one audit boundary. Reuses established Rust crates instead.

This project is GPL-3.0-or-later. Protocol names and behavior are interoperable
reimplementations; no upstream implementation source is vendored.

## Security boundaries

UDP discovery is untrusted. TCP initiators become TLS servers; TCP acceptors
become TLS clients. v8 exchanges identities again inside TLS and rejects changed
IDs/versions; v7 uses its legacy plaintext identity followed by TLS. Trusted v8
peers cannot downgrade to v7. Payload connections use TLS with the same pin.
Self-signed chain errors are allowed only because all application trust uses
explicit certificate pinning. OpenSSL still verifies handshake signatures;
missing certificates, invalid dates, CN mismatch and changed pins fail closed.

Only ping and battery are enabled after pairing. Notification, clipboard, share,
ring, media and input require per-device enablement. Input permission is temporary
and is reset at daemon startup and disconnect; a virtual uinput device exists
only while consent is active. No telemetry, cloud service, phone probing or
unsolicited network access is part of development or testing.

## Implementation boundaries and runtime resources

The library, daemon and CLI forbid handwritten unsafe Rust. The optional UI
uses qmetaobject's generated Qt FFI; that dependency necessarily contains unsafe
C++/Rust boundary code. The UI itself uses safe Rust APIs and moves blocking
D-Bus work onto a bounded worker queue. All product logic stays in Rust; the QML
files contain presentation and Qt clipboard/Content Hub adapters.

Main-channel frames are limited to 64 KiB and identities to 8 KiB, with handshake
and per-frame deadlines. There are at most sixteen handshakes/sessions, 64 online
devices, 64 queued packets per peer, two payload transfers, and 256 remembered
notification mappings. Transfer data stays in a private temporary file until the
exact declared length arrives; a failed/truncated transfer drops that file.
Payloads authenticate the same certificate as the control session and recheck
sharing consent while copying. Files are never executed; URLs are never launched
as a result of network packets. Notification markup is escaped before display.

Clipboard content is held only in RAM. The graphical-session agent uses Qt's
native clipboard implementation through a hidden QML TextEdit, polling only
when the daemon reports an opted-in online recipient. New notification monitoring
begins only with consent and closes when the last opted-in peer leaves. Monitor
permission failures are surfaced rather than taking over the notification name.
MPRIS exposes existing local players with a strict method allowlist. Remote input
uses evdev's safe uinput wrapper with bounded event conversion and balanced
press/release sequences; arbitrary shell commands, arbitrary keycodes and drag
holds are not accepted.

v8 verification hashes the two SubjectPublicKeyInfo DER public keys in descending
lexicographic order followed by the request's decimal timestamp. The first four
SHA-256 bytes are displayed as eight uppercase hex digits. An independent
OpenSSL-generated synthetic vector guards encoding and ordering compatibility.

## Deployment and verification

The NixOS module makes ingress firewall exceptions per named Wi-Fi interface;
it never adds global port exceptions. The daemon binds its TCP listeners and
outgoing connections to those interfaces' IPv4 addresses. UDP receive is bound
wildcard with subnet checks, so the module firewall is part of the ingress
interface boundary. Existing broad firewall policies remain the administrator's
responsibility. Interfaces are re-evaluated every ten seconds. IPv6/mDNS/routed
peers are deliberately not advertised as supported.

Phone integration (Mir clipboard, Content Hub, shell notifications, audio,
physical uinput and native aarch64 execution) requires the real-phone checklist
in README.md. Laptop tests exercise synthetic TLS peers, isolated D-Bus services,
private temporary files and synthetic sysfs battery fixtures. They do not scan
LAN devices, access a phone, inject events or play audio.

Pairing expiration additionally uses a monotonic deadline, so clock corrections
cannot extend consent. Clipboard reconnect packets compare timestamps before
replacing local text. A separate mapping of phone notification IDs lets desktop
dismissals close the original notification through the phone notification bus.
The isolated D-Bus test exercises outbound monitoring and this cancellation path.
