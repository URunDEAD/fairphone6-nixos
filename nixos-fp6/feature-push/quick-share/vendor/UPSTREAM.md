# Upstream sources

- `rqs_lib`: https://github.com/Martichou/rquickshare, commit
  `378d8ae969941bee4bf60ad34ac9cf8bb7005eb7`, `core_lib/`, version 0.11.5.
  GPL-3.0 (license retained). Only Rust sources/protobufs and build manifest
  are vendored; Tauri, generated TypeScript and release binaries are omitted.
- `mdns-sd`: https://github.com/Martichou/mdns-sd, branch `unsolicited`.
  Apache-2.0 OR MIT (licenses retained). Provides unsolicited announcement
  resend support used by rqs_lib. Exact revision recorded below.

Protocol research (no Swift code copied):
https://github.com/grishka/NearDrop/blob/add655c6c5a40b9ba03c0329b598f2817185f5a9/PROTOCOL.md
and rquickshare's README at the pinned revision above.

Local changes will be described in DESIGN.md and git history.
c3d6ec2e173ac2cf8306943f70026c37c2ab1dd7
