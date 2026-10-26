# FP6 Lomiri: Do Not Disturb, bedtime mode, notification history, quick replies (2026-10-26)

Module: `nixos-fp6/feature-push/notifications/notifications-fp6.nix`
(`hardware.fairphone6.notifications.enable`, on in `trial-lomiri-full-fp6.nix` with one
import line). Lomiri patches use their own overlay at `lib.mkOrder 2900`.

## What Lomiri 0.5 already had (checked in the sources)

* **Silent mode.** ayatana-indicator-sound's "silent-mode" action sets the AccountsService
  property `com.lomiri.touch.AccountsService.Sound.SilentMode`. telephony-service reads it
  (`GreeterContacts::silentMode()`): if it is set, `Ringtone` does not play the call or
  message sound, and it vibrates only if `Incoming*VibrateSilentMode` is set. That is all
  it does: there are no schedules, no exceptions, and Lomiri's own notification bubbles
  ignore it (`Notification.qml` plays every `sound-file` hint and wakes the screen for every
  bubble through `Powerd.setStatus`).
* **Notifications page.** System Settings > Notifications only has per-click-app
  sound/vibration lists and Emergency Broadcast. There are no click apps on this NixOS.
* **Notification server.** It is `lomiri-notifications` (QML plugin `Lomiri.Notifications`
  inside the Lomiri process; it owns `org.freedesktop.Notifications`). The model has no app
  name. There is no history: a bubble is gone once it closes. Only the messaging indicator
  keeps SMS and missed calls.
* **No DND** in Lomiri. ayatana-indicator-notifications (desktop DND) is not used on Lomiri.
* **Alarms** come from lomiri-indicator-datetime. They are snap decisions, and the indicator
  plays their sound itself, so they never depended on silent mode.

Silent mode stays as it is: it is independent of DND, still in the sound menu, and not
touched by DND.

## 1. Do Not Disturb (schedules, exceptions, visual behaviour)

Status: **done**. The daemon and its D-Bus API are live-tested on the phone. The Lomiri
bubble filter and the telephony hook are built. GUI round: see "Live tests".

How the user does it:
* **Status-bar menu:** a new "Do Not Disturb" indicator with the switches **Do Not
  Disturb** and **Bedtime mode**, plus "Do Not Disturb settings…". While DND (or bedtime)
  is on, the moon icon (`night-mode`) shows in the collapsed status bar. While it is off,
  the indicator appears only in the expanded menus.
* **System Settings > Do Not Disturb:**
  * the switch, with the current state ("On (Nights)");
  * **Schedules**: a list with a switch per schedule, add/edit/delete, name, days (the
    day the window starts), start and end. An end before the start means the next
    morning. Defaults: "Nights" every day 22:00-07:00 and "Weekdays" Mon-Fri 09:00-17:00,
    both off.
  * **Bedtime mode** (feature 2);
  * **Calls, messages and apps**: calls that ring (Everyone / Contacts / Favourite contacts
    / Nobody; default Favourites), **repeated callers** (a 2nd call from the same number
    within 15 min rings; default on), messages (same choices; default Favourites),
    numbers that always get through, and **apps whose notifications still show** (picked
    from the app names seen so far);
  * **Silenced notifications**: **Hide notification bubbles** (default on) and **Don't
    turn the screen on** (default on);
  * **Notification history** (feature 3).
* A manual switch lasts until the next schedule boundary (start or end). After that the
  schedules decide again, like on Android.
* Always through: alarms (snap decisions), the incoming-call screen (it is shown even when
  silenced, but without ringtone, vibration or screen wake), urgency-critical
  notifications, the Emergency SOS of the privacy-lock module (`fp6-privacy`, also
  whitelisted by name), and volume/brightness OSDs. Cell-broadcast emergency alerts are
  not touched.

Design:
* **fp6-notifyd** (Rust, user service, `fp6-notifyd/`). It computes the effective state
  from `dnd-manual` / `dnd-manual-since`, `dnd-schedules` (JSON), bedtime and its manual
  override, all in GSettings `com.fairphone6.notifications`. It writes `dnd-active`,
  `dnd-reason`, `bedtime-active` and `wallpaper-dim`, and re-evaluates every 20 s and on
  every settings change (local time via `localtime_r`, so TZ changes are followed). It
  serves `com.fairphone6.Dnd` with `ShouldAlert(kind, id, isContact, isFavourite) -> b`,
  `SetActive`, `SetBedtime`, the properties Active/Reason/Bedtime and a `Changed` signal.
  It also serves the Ayatana indicator `com.lomiri.indicator.dnd` (org.gtk.Menus/Actions).
  The journal shows only "DND call: allowed/silenced" and never the numbers.
* **Telephony hook** (`telephony-service-dnd.patch`, approved by the coordinator, and
  `cellular/` is not edited). The patch is on `pkgs.lomiri.lomiri-telephony-service`.
  `cellular/telephony-service-fp6.nix` overrides that package, so the `fp6-daemons` build
  that runs inherits it. In the built derivation the patch list is ucm-port-names →
  **telephony-service-dnd** → contacts-fallback. A header-only helper,
  `liblomiritelephonyservice/fp6dnd.h`, makes a synchronous call with a 400 ms timeout and
  no auto-start; if the daemon is missing or there is any error, it rings. It is used in:
  * `approver.cpp showSnapDecision`: no ringtone and no vibration when silenced (the
    call-waiting tone stays), and the hint `x-fp6-dnd=silenced|allowed`;
  * `textchannelobserver.cpp showNotificationForMessage`: the message sound and vibration
    (`Ringtone::playIncomingMessageSound` vibrates too) only when allowed, the same hint,
    and also the "already read" path.
  The favourite flag comes from the contact the approver/indicator already matched
  (`QContactFavorite`).
* **Lomiri** (`lomiri-dnd.patch`, QML only):
  * `Notifications.qml`: a `GSettings` object and `fp6Silenced(hints, type, urgency)`.
  * `Notification.qml`: decides once per bubble. When silenced: no sound (`sound-file`
    dropped). If "hide bubbles" is on, the bubble is closed at once (the sender gets
    NotificationClosed, the history still records it). If "don't wake" is on, there is no
    `Powerd.setStatus(On)`, and the matching Off is only sent when an On was sent.
  * `Wallpaper.qml`: the bedtime dim rectangle.
* **lomiri-notifications** (`lomiri-notifications-app-name.patch`): `Notify` copies
  `app_name` into the hint `x-fp6-app-name`, so the shell can match allowed apps. The
  upstream unit test expectation is updated in the same patch (`GetNotifications` returns
  hints).

Limits (by design or not done):
* USSD, flash SMS and "joined group" notices of the telephony indicator still play their
  sound (rare, not patched). Missed-call/voicemail bubbles follow the app rule ("Lomiri
  Telephony Service Indicator"). The telephony hint is set only on calls and SMS.
* "Repeated callers" counts calls the approver showed while fp6-notifyd was running
  (in-memory, 15 min).
* Any session process can call `ShouldAlert`, which records a "call" for the
  repeated-callers rule. This is only within the user's own session.

## 2. Bedtime mode

Status: **done** (daemon side live-tested, wallpaper dim built; GUI round below).

* Settings > Do Not Disturb > Bedtime mode:
  * **On a schedule** (default off, 22:30-07:00, every day);
  * **Bedtime now** (also in the status-bar menu; lasts until the next bedtime boundary);
  * **During bedtime**: Do Not Disturb (default on), **Greyscale screen** (default on),
    **Dim the wallpaper** (default on, 60 %, slider 10-90 %).
* Alarms still ring: DND never silences snap decisions, and indicator-datetime plays its
  own sound.
* Greyscale: this uses the display-comfort agent's interface (relayed by the coordinator):
  GSettings `com.fairphone6.display-comfort`, key `greyscale-active`. fp6-notifyd writes it
  with `dconf write /com/fairphone6/display-comfort/greyscale-active true` at bedtime
  start. The value from before is kept in `~/.local/state/fp6-notifyd/state.json` and is
  restored at the end ("unset" means `dconf reset`). It never writes `night-light-current`.
  The key path is the module option `greyscaleKey`; empty means bedtime never touches
  colours.
* Dim wallpaper: a black rectangle in Lomiri's `Wallpaper` component (desktop, greeter,
  lock screen), with opacity `wallpaper-dim` and a slow fade. Display-comfort's overlay is
  in ShellScreen.qml, so the two do not overlap.

## 3. Notification history

Status: **done** (observer and store live-tested on the phone; the Settings viewer is
built).

* **Default OFF.** Settings > Do Not Disturb > Notification history:
  * **Keep a notification history** (turning it off deletes the history);
  * **Also keep the text** (default off; turning it off removes the stored texts);
  * **Keep for** 1 / 7 (default) / 30 / 90 days;
  * **Clear history** (with confirmation);
  * the list, newest first: icon, title, app, time, "during Do Not Disturb", and the text
    only if kept. Swipe an entry → "Don't record this app" (an exclude list, reversible).
* Observer: fp6-notifyd opens a second session-bus connection and calls
  `org.freedesktop.DBus.Monitoring.BecomeMonitor` with the single rule
  `type='method_call',interface='org.freedesktop.Notifications',member='Notify'`. It only
  reads. It works on the phone's dbus-broker (live), and it cannot block or change
  notifications. OSD bubbles (`x-lomiri-private-synchronous`) are skipped. If `app_name`
  is empty, the sender's `/proc/PID/comm` is used.
* Store: `~/.local/share/fp6-notifyd/history.jsonl` (mode 0600, directory 0700),
  append-only, rewritten when pruned (retention, at most 5000 entries). The Settings page
  reads `$XDG_RUNTIME_DIR/fp6-notifyd/status.json` (0600, on tmpfs). That file holds the
  last 1000 entries only while the history is on, plus the app names seen.
* App names seen (only names, for the "apps allowed" picker) are kept in
  `~/.local/state/fp6-notifyd/apps.json` even while the history is off.

## 4. Quick replies beyond SMS (feasibility only)

* **SMS already has it**, but not in the bubble. The Messages entry in the status-bar
  messaging menu (ayatana-indicator-messages, fed by lomiri-indicator-telephony-service)
  has a **quick-reply text field** (`quickReply` → `MessagingMenu::sendMessageReply`). A
  missed call has "reply with message" (`replyWithMessage`). The incoming-call snap
  decision has "Message & decline" with preset texts. These all go through the existing
  telephony patches, and DND does not change them.
* **Bubbles:** lomiri-notifications advertises `actions`, `body`, `body-markup`, the value,
  sound and urgency hints and the Lomiri-private hints, but **not** `inline-reply`. A
  bubble can only have action buttons (Interactive: exactly 2; snap decisions: 2-14). Text
  input in a bubble exists only through `x-lomiri-private-menu-model`: the sender exports
  a GMenuModel + GActionGroup on D-Bus with a `com.canonical.snapdecision.textfield` item.
  Telephony uses this for its auth prompts.
* **Generic apps:**
  * KDE (`x-kde-reply-*` hints + the `inline-reply` action + `NotificationReplied`) and
    GTK/portal apps would need about 150 lines: the server advertising `inline-reply`,
    emitting `NotificationReplied(id, text)`, and a TextField in `Notification.qml` for an
    `inline-reply` action.
  * No app on this phone sends such notifications: the Qt5 Lomiri apps use libnotify
    without reply, Firefox/Morph do not use it, and Waydroid's Android notifications are
    not bridged.
  * So there is no benefit now: **feasible, not implemented**. A per-app reply via
    `x-lomiri-private-menu-model` would mean changing each app.

## Files

`nixos-fp6/feature-push/notifications/`:
* `notifications-fp6.nix`: the module (options `enable`, `greyscaleKey`, `telephony`).
* `com.fairphone6.notifications.gschema.xml`, `schema.nix`.
* `fp6-notifyd/` (Rust: `main.rs`, `policy.rs`, `schedule.rs`, `history.rs`, `monitor.rs`,
  `indicator.rs`; 10 unit tests, which run in the Nix build) and `package.nix`.
* `lomiri-dnd.patch`, `lomiri-notifications-app-name.patch`, `telephony-service-dnd.patch`.
* `settings-page/` (PageComponent, Schedules, ScheduleEdit, Exceptions, Bedtime, History,
  DaysRow, TimeDialog, DndModel) and `lomiri-system-settings-dnd.nix`.
* `test/daemon-live-test.sh` (daemon on the phone, no Lomiri change) and
  `test/gui-round.sh` (test Lomiri round).

## Builds
