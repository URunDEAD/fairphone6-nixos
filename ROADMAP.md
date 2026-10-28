# Roadmap

Planned features for the Fairphone 6 NixOS + Lomiri port, in priority order.
Every user-facing feature gets a setting/toggle in System Settings. Status:
✅ done · 🔧 in progress · ⏳ planned · ❓ needs the user / carrier.

## Priority 1 — quality-of-life (current focus)

### Sensor gestures (in the Gesture Shortcuts settings page)
- ✅ Shake to … (flashlight / app / command), off by default
- ✅ Power button double/triple press shortcuts (none by default)
- ✅ Pocket mode — ignore touches and power presses in a pocket (proximity + light)
- ✅ Flip to silence — face down silences calls/notifications
- ✅ Raise to wake / lift to check — screen on when picked up
- 🔧 Back tap (experimental, needs calibration) (double/triple) shortcuts — accelerometer
- ✅ Volume keys long-press = next/previous track while the screen is off

### Display
- ✅ Night light / blue-light filter — scheduled (sunset/sunrise or times), strength
- ✅ Dark theme on a schedule
- 🔧 Font size, bold text and display scaling — in r5, awaiting test

### Battery and charging
- ✅ Charge limit (e.g. 80 %) to protect the battery
- ✅ Adaptive charging — finish at 100 % just before the alarm
- ✅ Battery saver mode — CPU limits, background apps, sync, brightness
- ✅ Battery health and usage — wear, cycles, per-app usage
- ❓ Suspend (UFS fix built; needs a supervised test) — biggest battery win

### Lock screen, notifications, privacy
- ✅ Auto-lock — timeout and lock on power press (verify and expose)
- ✅ Emergency info on the lock screen, SOS shortcut, location sharing
- ✅ Do Not Disturb with schedules and exceptions; bedtime mode
- ✅ Notification history
- ✅ Camera / microphone / location in-use indicators in the status bar
- ⏳ Clipboard privacy (auto-clear, read notifications) — blocked: Mir 2.15 has no
  data-control protocol; needs a Mir upgrade (see below) or a backport
- ✅ Wi-Fi MAC address randomisation

## Priority 2 — system

- 🔧 Upgrade Mir (2.15 → current, e.g. 2.28 in nixpkgs) for Lomiri/qtmir —
  brings data-control (clipboard privacy), viewporter/fractional scale (removes the
  Firefox scaling shim) and newer Wayland protocols; needs qtmir porting

- ✅ Over-the-air updates (Settings > System update, settable server URL; single-slot
  for now — automatic A/B rollback waits for a supervised slot test)
- 🔧 Backup and restore (settings, messages, contacts, photos; encrypted)
- 🔧 Storage manager
- 🔧 Adaptive screen timeout (stay on while held) — in r4, awaiting test
- ⏳ Always-on display (measure battery cost first)
- 🔧 Refresh rate / Smooth display: panel already runs 120 Hz; 30/60/90/120 Hz +
  adaptive (120 touched, 60/30/10 idle) + battery-saver cap + tile built, kernel
  patches 0009/0010 are candidates; off until a supervised RAM-boot test
- 🔧 Screen-edge notification flash (the FP6 has no LED) — in r4, awaiting test

## Priority 3 — connectivity and integration

- ❓ VoLTE (implemented; waiting for Orange provisioning), then Wi-Fi calling
- 🔧 KDE Connect integration (notifications, clipboard, files, remote input) — in r4
- 🔧 Nearby file sharing: Quick Share to Android (Rust) and a web-share fallback
  for iPhones — in r4. AirDrop is not possible (the Wi-Fi chip has no NAN/monitor mode)
- ⏳ Research: CallKit-style integration for internet calls — third-party
  VoIP/messenger calls (native Linux apps via a D-Bus API, and Waydroid apps
  such as WhatsApp/Signal via Android's ConnectionService) get the same
  incoming-call screen over the lock screen, ringtone/DND rules, in-call
  audio routing, proximity handling and call history as cellular calls
  (Lomiri telephony-service / Telepathy as the common layer)
- 🔧 Unified system share sheet (first version in r4) — — one consistent "Share" panel for
  all apps (iOS ShareSheet style), built on Lomiri's content-hub (already a
  system-wide share/peer picker for Ubuntu Touch apps) and extended to GTK,
  Qt6 and Firefox apps (via an xdg-desktop-portal backend) and to Waydroid
  apps (Android share intents ↔ content-hub), with share targets/extensions,
  recent contacts ("direct share") and actions like copy, save to files,
  print, Nearby share, KDE Connect
- 🔧 Smart Wi-Fi ↔ mobile switching, data saver (per-app network policy) — services in r6;
  blocking needs a kernel with nft socket matching (built, waiting for a supervised RAM-boot test)
- 🔧 Bluetooth codecs and quick pairing review — in r5, awaiting test
- ⏳ NFC tag reader/writer app (no payments)
- ⏳ Research: system authentication API (like iOS LocalAuthentication / Android
  BiometricPrompt) — apps ask the system to confirm the user (fingerprint via
  biometryd, or passcode) through one standard system dialog and only get a
  yes/no; optionally keys that unlock only after authentication, kept in a
  hardware-backed keystore if the Qualcomm TEE allows it, else a sealed
  software keystore. Scope: the device owner's own security for native apps;
  not proving device integrity to other apps, and not bypassing their checks
- 🔧 Per-app permissions: USB/NFC and network prompts in r5; camera, microphone and
  location prompts in progress
- ⏳ Encrypted user data
- ⏳ Work profile / app separation

## Priority 4 — accessibility and smart features

- 🔧 Magnification gesture, colour correction/inversion, mono audio and balance — in r4
- ⏳ Screen reader improvements
- 🔧 On-device voice dictation with a keyboard mic key — in r5, awaiting test
- 🔧 Keyboard prediction quality, Romanian support — in r4
- 🔧 Text recognition (OCR) in photos and screenshots — in r4
- 🔧 Live captions (offline, English/Romanian) — in r4

## Dedicated projects (later, separately)

- ⏳ Document scanning (camera) — to be done as its own focused project

## Recently added (in r4, awaiting the hardware test)

- 🔧 Android/iOS-style control centre: editable tiles, hold a tile for details
- 🔧 Audio output/input switching, automatic on connect, per-app routing
- 🔧 Bluetooth settings: pairing on/off, rename, device type/icon, paired list
- 🔧 USB connection mode chooser (charge, reverse charge, file transfer, network; combinable)
- 🔧 Call recording (uplink and downlink as separate files)
- 🔧 State that survives reboots (VPN and similar, each with a toggle)
- 🔧 Wi-Fi sharing and joining by QR code
- 🔧 Separate lock and home wallpapers; side-button fingerprint hint on the lock screen
- 🔧 App switcher: long swipe at the leftmost card closes all apps
- 🔧 System file picker for GTK/Firefox/portal apps
- 🔧 Faster modem start (saved modem storage copy)
- 🔧 Bug report tool and a post-update self-test

## Added in r5 and r6 (awaiting the hardware test)

- 🔧 Lock and unlock sounds, with a setting and a choice of sound (r5)
- 🔧 Charging feedback on plug-in, also on the lock screen (r5)
- 🔧 App crash isolation: a crashing app no longer takes the shell or the session
  with it; the shell restarts without logging out; crash notice with a bug report (r5)
- 🔧 Notification quick replies (r5)
- 🔧 Assistant app: chat with an on-phone or remote AI model that can use system apps
  after asking for each resource (r6)
- 🔧 On-phone AI models: download, verify, choose, delete (r6)
- 🔧 Alarms ring reliably through the shared alarm store (r6)

## In progress elsewhere

- ✅ Camera: fast launch and lock-screen secure mode
- ⏳ Camera: full field of view for main/ultrawide photos (today a 2× centre crop:
  the soft ISP can't scale — needs a binning/scaling path or full-size stream)
- ✅ Hardware video encoder (iris) works (used by screen recording); camera use next: — enables faster timelapse, screen recording,
  slow motion (ultrawide 120 fps mode) and electronic stabilisation (gyro)
- ✅ Camera Phase 2: RAW/DNG, HDR, night mode, timelapse, bracketing,
  intervalometer, panorama (Rust), front/ultrawide colour calibration, mirror selfie
- ✅ Keep apps ready (prelaunch, preload, zram, OOM policy); memory profiles
  (Conservative/Balanced/Aggressive) with usage prediction; kept-ready apps stay resident (r6)
- 🔧 Sensor utility apps (Tools folder): batch 1 done, batch 2 in progress
- ✅ Screenshots and screen recording
- ✅ USB Drive app (DriveDroid-like: share ISO/disk images over USB)
- ❓ Supervised suspend test, 24 h battery and 7-day stability runs
