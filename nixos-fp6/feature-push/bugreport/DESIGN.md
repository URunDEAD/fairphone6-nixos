# Design

## Components and trust boundaries

`config` validates the same options for CLI and QML. `collect` reads fixed files and runs fixed commands through an injectable `Sources`/`Runner` interface. `redact` is a reusable pure function, independent of Qt, compression or hardware. `bundle` publishes the private archive and summary. No application-authored unsafe Rust is needed in the collector/helper; the Qt bridge uses qmetaobject's generated Rust/C++ FFI and Qt's own native implementation.

The GUI is a thin Rust QObject bridge plus Lomiri UI Toolkit 1.3 QML. Its worker thread owns collection; Qt polls a Rust channel without touching a QObject from the worker. Options are snapshotted before the worker starts. A systemd **user** service owns the GUI process and prevents duplicate instances from the desktop launcher. It has no `WantedBy`, so it does not start at login. There is no backend TCP listener, no network API, no new D-Bus daemon, and no persistent preferences or GSettings schema. Labels use anchors/margins, not unsupported UITK padding properties.

The privilege boundary is a separate non-setuid Rust executable, invoked once through `pkexec` when restricted sources are explicitly selected. The installed polkit action is bound to its exact immutable store path, requires `auth_admin` for an active session, and denies inactive/remote sessions. It does not cache authorization. The helper verifies its root identity and accepts only a bounded relative time window and a subset of four source names. It has no caller-supplied executable, output path, file path, shell fragment, journal unit, or arbitrary journal filter. It emits redacted JSON to stdout and never writes as root. The user process validates the helper response and redacts it again.

Nix pins collector executable paths to dependencies at build time. The helper never resolves commands through caller-controlled PATH. Children get a minimal environment and fixed locale with no pager. Stderr is discarded because error messages can repeat private arguments. Exit failures and missing tools are reported with fixed messages. An ordinary user directly launching the helper is refused; a root administrator is already outside this privilege boundary.

## Collection policy

All sections start selected, but restricted data and images start unselected. A disabled section is never probed. Files are a fixed allowlist: release/uptime/kernel, memory/pressure/zram, and typed battery/thermal properties. No environment dump, full configuration tree, contact database, SMS, core file, saved connection profile, or filesystem traversal is collected. NetworkManager fields deliberately omit connection names and SSIDs. oFono JSON is decoded and only known boolean states, enumerated registration/technology/PIN states and bounded signal strength survive; modem object paths are used for follow-up calls but are not saved.

Journal streams are line-bounded (3,000 normally, smaller for filtered diagnostics), time-bounded (1 minute–7 days), and byte-bounded. Each normal command has a 12-second deadline and 1 MiB retained stdout limit. The privileged aggregate has 120 seconds and an 8 MiB response limit, allowing four separately bounded sources and JSON escaping. The fixed tools do not daemonize; stdout draining runs concurrently to avoid pipe deadlock. A truncated final line is discarded so a cut-off sensitive field is not persisted. Kernel ring-buffer collection is bounded by bytes, not `--since`; the summary and README make that distinction.

Unavailable or partial sources remain visible in summary status. A source returning success with no messages is distinguishable from a missing command. The system journal may expose only what the invoking account can read; requesting restricted logs explicitly adds the helper's output. Metadata remains useful but is not treated as a complete view of the device.

## Redaction model

Text exists transiently in process memory. Redaction happens before entries are returned by the collector, before privileged data leaves the helper, and again at the persistence boundary. Secret-bearing lines are removed wholesale so quoted, spaced and multiword values cannot leak through a narrow value regex. IPv6 candidates are validated using the standard library, including compressed, scoped and IPv4-mapped forms. Other rules conservatively remove phone/device-number patterns, MACs, IPv4, email, common coordinate forms, SSID/BSSID lines, keys and credentials. ANSI/control normalization precedes matching.

This deliberately trades some diagnostics for privacy: large counters can resemble identifiers, and a whole log line can disappear. It cannot guarantee anonymity for arbitrary prose, undocumented formats, encoded data, usernames/hostnames outside known patterns, or secret-bearing images. We explain this in the app and README instead of claiming universal scrubbing. Tests use synthetic identities and injected fake collectors; laptop or phone data is not a test fixture.

## Archive lifecycle

The default is the literal `~/Documents/Bug reports`, with ownership/type/mode checks. The reports root and randomized staging directories are 0700. Archive and summary are created with `create_new` and 0600, independently of umask. Tar members have 0600 permissions, no owner names and zero UID/GID/mtime. Names are fixed relative paths and checked against traversal.

Only redacted text is streamed to zstd; no raw text staging files are created. After compression and both file syncs succeed, a directory rename publishes the complete report, including its sibling summary. A failed normal call drops the temporary directory. A process kill or power loss can leave a private `.pending-*` directory; no report is announced before publication. File sync is used, but the parent directory is not fsynced, so a sudden power loss immediately after publication can lose the directory entry. This is a local diagnostic artifact, not a durable transactional database.

The root directory must be user-owned and private; another process running as the same user is not a separate security boundary. Symlink checks defend against preexisting output redirection, not a hostile same-UID process racing every operation. No retention cleanup or deletion of prior reports occurs automatically.

## Capture and shell integration

Capture now is an immediate snapshot with an independently consented, unredacted screenshot. The exact assumed GNOME-compatible session D-Bus contract is documented in README. Availability is determined by making the requested call; no live phone probing was done. An absent interface is a normal partial result. Screenshot data is read only from the new private requested filename, never from a returned arbitrary path. The temporary directory is private even if the shell creates its PNG with a permissive file mode. Completion removes it; abnormal termination may leave it private until the user or OS cleans temporary files.

The QML capture action minimizes the report window and waits 500 ms before calling the collector; the window returns when collection finishes. That transition must be checked against the actual phone compositor. A shortcut invoking the CLI can capture the affected app without changing focus.

Proposed **quick-settings tile**: a momentary “Bug report” action closes the panel, then opens the report app to the preview. A long press can offer “Capture logs now” and a separate “Capture with screenshot” confirmation. The latter invokes `fp6-bugreport --capture-now --screenshot --sections display,journal,memory` only after the panel has closed. Show a local completion notification with the report directory, not an automatic share sheet.

Proposed **power-menu item**: “Create bug report” next to existing utility actions, launching the same user service. Never capture automatically on power-button presses, never grant log privileges implicitly, and never add screenshots to a one-tap default action. These are integration proposals, not patches to a shell that is outside this repository.

## Validation strategy

Run synthetic unit/integration tests on x86_64 without Qt by default, then build the real Qt-enabled binary and Nix package. Archive tests inspect the decompressed tar, permissions, traversal rejection, unique paths, consent checks and cleanup. Fake source tests verify section gating, journal arguments, coredump-list-only behavior, oFono field selection and helper restrictions. Subprocess tests cover timeout/failure/output limits with synthetic shell output. QML previews use the installed UITK under the offscreen platform; preview mode cannot collect.

NixOS module evaluation checks enablement, policy exposure and the on-demand user service. Buildability on x86_64 does not establish runtime correctness on the phone. Authentication, shell-specific screenshot IPC, rendering transitions, real unit names and hardware sysfs layout require the manual checklist in README.
