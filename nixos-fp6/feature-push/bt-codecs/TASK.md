# Task: Bluetooth audio codecs and quick pairing review (Rust + QML)

Read ../RULES.md fully first. Deliver in this directory.

Goal: on the Fairphone 6 (PipeWire + WirePlumber + BlueZ), let the user see and choose the
Bluetooth audio codec and profile per device, and review/improve the pairing flow.

1. Codecs: make sure the system's PipeWire/WirePlumber build enables the BlueZ codecs that
   nixpkgs can ship (SBC, SBC-XQ, AAC, aptX/aptX HD, LDAC, LC3, mSBC/LC3-SWB for calls; check
   which spa-bluez5 codec plugins the pinned nixpkgs actually builds, and say which need an
   override). Provide a NixOS module option set under hardware.fairphone6.bluetoothAudio.*
   (enable, preferred codec order, hardware-volume, auto-switch to HFP when a call starts).
2. A small Rust helper/daemon (zbus, D-Bus name com.fairphone6.BluetoothAudio1, user session)
   that lists connected BT audio devices with current codec, available codecs, profile
   (A2DP / HFP), bitrate where known, and can switch codec/profile via PipeWire
   (pw-cli / libpipewire or WirePlumber's "bluez5.codec" props; prefer a clean API usage,
   no shell-outs unless justified). Read-only status calls must not change state.
3. QML (Lomiri UI Toolkit 1.3) additions for the existing Settings > Bluetooth device page:
   a "Audio quality" section (codec picker, "prefer quality / prefer stable connection"
   toggle, current bitrate). The existing Bluetooth settings code is in
   nixos-fp6/feature-push/bluetooth-settings/ and audio routing in
   .../audio-routing/ (read-only: put your proposed patch to those files as a .patch file here,
   explained in FINAL_REPORT.md).
4. Quick pairing review: read the bluetooth-settings code and write REVIEW.md with concrete
   issues and proposed fixes (pairing prompts, PIN/passkey confirmation UI, reconnect behaviour,
   timeouts); implement the low-risk fixes as patches here.

Tests: cargo test with a mocked PipeWire/D-Bus layer; QML previews offscreen into previews/.
Package as package.nix + module.nix like ../selftest. Synthetic device names only
("Test Speaker", "Test Headphones"), synthetic MAC 00:00:00:00:00:01. Nothing security related
(no pairing bypasses, no sniffing). Finish with FINAL_REPORT.md.
