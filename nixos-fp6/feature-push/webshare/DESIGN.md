# WebShare design

The Rust daemon owns one temporary share at a time. The session bus API is
`com.fairphone6.WebShare1`, at `/com/fairphone6/WebShare1`. The CLI uses that API.
The Lomiri QML front end uses a separate authenticated loopback control endpoint
because stock Qt 5 QML does not expose QtDBus. Its per-daemon secret lives only
in a mode-0600 file inside the user's mode-0700 runtime directory. No public
endpoint can approve uploads, choose files, or read the local clipboard.

## Network and invitation lifecycle

This implementation chooses the task's HTTP-on-LAN option. It does not offer
TLS or claim encryption: both UIs warn that a trusted Wi-Fi or phone hotspot is
required. A fresh 128-bit random invitation is encoded in a short URL and QR.
Opening the URL shows a join form; POST consumes the invitation atomically and
sets a separate random HttpOnly SameSite=Strict browser cookie. Subsequent
visitors cannot claim it. One browser can fetch multiple explicitly shared files.
No external resources, analytics, DNS discovery, or third-party QR service.

Only RFC1918 IPv4 addresses on kernel-recognized wireless interfaces are
eligible; hotspot interfaces with `phy80211` are included. No wildcard,
Ethernet, cellular, VPN, public, IPv6, or loopback share listeners. Peers must
be in the selected subnet. The listener is stopped on expiry, explicit stop,
interface/address loss, or completion. The fixed port is 8765, so a firewall
exception can be scoped to an explicitly configured Wi-Fi interface. The app
never creates hotspots or changes network/firewall configuration itself.

## File and consent boundaries

Share only regular files explicitly selected by the local user, opened with
no-follow semantics and held open for the session. Download URLs use numeric
IDs, never paths. Network names are metadata only, never filesystem paths.
Uploads are disabled by default and streamed to private random staging files
with a hard byte limit, bounded concurrency and pending count. Each upload
needs a local approval; rejection, stop, expiry, and daemon crash remove staging
files. Approval persists to a generated unique name with no-clobber semantics.
Uploads never become downloadable through the sharing endpoint.

Text is separately opt-in, bounded, held in memory, and shown on the phone for
an explicit Copy action. Text is never executed or opened as a URL. The QML
process owns clipboard interaction. The daemon never reads clipboard contents.

## Resource limits and completion

Sessions last 1–30 minutes (10 by default). There are at most 32 shared files,
8 pending uploads, 4 simultaneous data transfers, 100 MiB per upload, and 4 KiB
of pending text. HTTP request rate and body deadlines bound abusive clients.
All authenticated requests require the bound Host; mutation requests also
reject foreign Origin and cross-site Fetch Metadata. Responses use no-store,
no-referrer, CSP, and nosniff; downloads force attachment/octet-stream.

After all selected files have streamed successfully, a download-only session
stops automatically. A receive-only session stops after the first approved
upload or copied text. Mixed send/receive sessions stay open until explicit
"Finish sharing", local Stop, or expiry, so downloads do not discard uploads
awaiting approval. An interrupted download remains retryable. Completion means
bytes consumed by the HTTP server; no network protocol can prove the browser
saved them to durable storage. Range/resume is intentionally unsupported.

## Trust and scope

The same logged-in OS account and session bus are trusted. HTTP does not protect
against a malicious LAN peer sniffing an invitation/cookie or transferred data.
Use a private hotspot for sensitive material; application-layer TLS is a future
extension. The listener uses Linux SO_BINDTODEVICE plus address binding and subnet checks.
Device binding failure is fatal; it never falls back to address-only binding.
Keep a firewall on multihomed systems. Physical phone/hotspot/Safari and content-hub
integration need the manual device checks in README.md; laptop tests use only
synthetic files and loopback transports and never access a phone.
