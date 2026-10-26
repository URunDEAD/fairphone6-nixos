# Roadmap

Planned features for the Fairphone 6 NixOS + Lomiri port, in priority order.
Every user-facing feature gets a setting/toggle in System Settings. Status:
✅ done · 🔧 in progress · ⏳ planned · ❓ needs the user / carrier.

## Priority 1 — quality-of-life (current focus)

### Sensor gestures (in the Gesture Shortcuts settings page)
- ✅ Shake to … (flashlight / app / command), off by default
- ✅ Power button double/triple press shortcuts (none by default)
- ⏳ Pocket mode — ignore touches and power presses in a pocket (proximity + light)
- ⏳ Flip to silence — face down silences calls/notifications
- ⏳ Raise to wake / lift to check — screen on when picked up
- ⏳ Back tap (double/triple) shortcuts — accelerometer
- ⏳ Volume keys long-press = next/previous track while the screen is off

### Display
- ⏳ Night light / blue-light filter — scheduled (sunset/sunrise or times), strength
- ⏳ Dark theme on a schedule
- ⏳ Font size and display scaling settings

### Battery and charging
- ⏳ Charge limit (e.g. 80 %) to protect the battery
- ⏳ Adaptive charging — finish at 100 % just before the alarm
- ⏳ Battery saver mode — CPU limits, background apps, sync, brightness
- ⏳ Battery health and usage — wear, cycles, per-app usage
- ❓ Suspend (UFS fix built; needs a supervised test) — biggest battery win

### Lock screen, notifications, privacy
- ⏳ Auto-lock — timeout and lock on power press (verify and expose)
- ⏳ Emergency info on the lock screen, SOS shortcut, location sharing
- ⏳ Do Not Disturb with schedules and exceptions; bedtime mode
- ⏳ Notification history
- ⏳ Camera / microphone / location in-use indicators in the status bar
- ⏳ Clipboard privacy (auto-clear, read notifications) — blocked: Mir 2.15 has no
  data-control protocol; needs a Mir upgrade (see below) or a backport
- ⏳ Wi-Fi MAC address randomisation

## Priority 2 — system

- ⏳ Upgrade Mir (2.15 → current, e.g. 2.28 in nixpkgs) for Lomiri/qtmir —
  brings data-control (clipboard privacy), viewporter/fractional scale (removes the
  Firefox scaling shim) and newer Wayland protocols; needs qtmir porting

- ⏳ Over-the-air updates with automatic rollback (same safe boot procedure)
- ⏳ Backup and restore (settings, messages, contacts, photos; encrypted)
- ⏳ Storage manager
- ⏳ Adaptive screen timeout (stay on while held)
- ⏳ Always-on display (measure battery cost first)
- ⏳ Higher refresh rate if the panel supports it
- ⏳ Notification quick replies beyond SMS
- ⏳ Screen-edge notification flash (the FP6 has no LED)

## Priority 3 — connectivity and integration

- ❓ VoLTE (implemented; waiting for Orange provisioning), then Wi-Fi calling
- ⏳ KDE Connect integration (notifications, clipboard, files, remote input)
- ⏳ Nearby file sharing
- ⏳ Research: CallKit-style integration for internet calls — third-party
  VoIP/messenger calls (native Linux apps via a D-Bus API, and Waydroid apps
  such as WhatsApp/Signal via Android's ConnectionService) get the same
  incoming-call screen over the lock screen, ringtone/DND rules, in-call
  audio routing, proximity handling and call history as cellular calls
  (Lomiri telephony-service / Telepathy as the common layer)
- ⏳ Research: unified system share sheet — one consistent "Share" panel for
  all apps (iOS ShareSheet style), built on Lomiri's content-hub (already a
  system-wide share/peer picker for Ubuntu Touch apps) and extended to GTK,
  Qt6 and Firefox apps (via an xdg-desktop-portal backend) and to Waydroid
  apps (Android share intents ↔ content-hub), with share targets/extensions,
  recent contacts ("direct share") and actions like copy, save to files,
  print, Nearby share, KDE Connect
- ⏳ Smart Wi-Fi ↔ mobile switching, data saver
- ⏳ Bluetooth codecs and quick pairing review
- ⏳ NFC tag reader/writer app (no payments)
- ⏳ Research: system authentication API (like iOS LocalAuthentication / Android
  BiometricPrompt) — apps ask the system to confirm the user (fingerprint via
  biometryd, or passcode) through one standard system dialog and only get a
  yes/no; optionally keys that unlock only after authentication, kept in a
  hardware-backed keystore if the Qualcomm TEE allows it, else a sealed
  software keystore. Scope: the device owner's own security for native apps;
  not proving device integrity to other apps, and not bypassing their checks
- ⏳ Per-app permissions (camera, microphone, location, files) on top of AppArmor
- ⏳ Encrypted user data
- ⏳ Work profile / app separation

## Priority 4 — accessibility and smart features

- ⏳ Magnification gesture, colour correction/inversion, mono audio
- ⏳ Screen reader improvements
- ⏳ On-device voice dictation in the keyboard
- ⏳ Keyboard prediction quality, Romanian support
- ⏳ Text recognition (OCR) in photos and screenshots
- ⏳ Live captions (heavy; research)

## Dedicated projects (later, separately)

- ⏳ Document scanning (camera) — to be done as its own focused project

## In progress elsewhere

- 🔧 Camera: fast launch
- ⏳ Camera: full field of view for main/ultrawide photos (today a 2× centre crop:
  the soft ISP can't scale — needs a binning/scaling path or full-size stream)
- ⏳ Hardware video encoder (iris) — enables faster timelapse, screen recording,
  slow motion (ultrawide 120 fps mode) and electronic stabilisation (gyro)
- ✅ Camera Phase 2: RAW/DNG, HDR, night mode, timelapse, bracketing,
  intervalometer, panorama (Rust), front/ultrawide colour calibration, mirror selfie
- 🔧 Keep apps ready (prelaunch, preload, zram, OOM policy)
- 🔧 Sensor utility apps (Tools folder), batch 2 next
- 🔧 Screenshots and screen recording
- ❓ Supervised suspend test, 24 h battery and 7-day stability runs
