// SPDX-License-Identifier: MIT
//! App identity from systemd unit names and cgroup paths.
//!
//! Used for the per-app network permission (DESIGN.md "App network
//! permission"): app ids in /run/fp6-permissions/network.json are matched to
//! cgroups by unit name. Same code as app-permissions/rust/src/appid.rs (the
//! broker's caller identity); keep the two in sync, the test vectors are
//! identical. Two launch conventions are recognised:
//!
//!  * lomiri-app-launch (today): every app is a transient user service
//!    `lomiri-app-launch--<job>--<appid>--<instance>.service` in the user
//!    manager's app.slice (job `application-legacy`, `application-click`,
//!    `application-snap`, `untrusted-helper`, ...).
//!  * freedesktop / systemd "desktop environment" convention (what a launcher
//!    that puts every app in its own scope uses, e.g. the crash-isolation
//!    work): `app[-<launcher>]-<ApplicationID>-<RANDOM>.scope` and
//!    `app[-<launcher>]-<ApplicationID>[@<RANDOM>].service`, where dashes
//!    inside the id are escaped as `\x2d`.
//!
//! The app id is the desktop-file id (`morph-browser`,
//! `de.haeckerfelix.Fragments`), the same id Lomiri's ApplicationManager
//! reports as focusedApplicationId and the AppArmor profile name.

/// systemd unit-name unescaping (`\xNN`).
pub fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Ids we accept: desktop-file ids (no slashes, no spaces, bounded).
pub fn valid_app_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 180
        && !s.starts_with('.')
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

fn checked(id: String) -> Option<String> {
    valid_app_id(&id).then_some(id)
}

/// App id of one unit name (the last component of a cgroup path), if the
/// unit is an app unit.
pub fn app_id_from_unit(unit: &str) -> Option<String> {
    if let Some(rest) = unit.strip_prefix("lomiri-app-launch--") {
        let rest = rest.strip_suffix(".service")?;
        let (_job, rest) = rest.split_once("--")?;
        let (app, _instance) = rest.rsplit_once("--")?;
        return checked(unescape(app));
    }
    if let Some(rest) = unit.strip_prefix("app-") {
        if let Some(body) = rest.strip_suffix(".scope") {
            // app[-launcher]-ID-RANDOM.scope
            let parts: Vec<&str> = body.split('-').collect();
            let id = match parts.len() {
                2 => parts[0],
                3 => parts[1],
                _ => return None,
            };
            return checked(unescape(id));
        }
        if let Some(body) = rest.strip_suffix(".service") {
            // app[-launcher]-ID[@RANDOM].service
            let body = body.split_once('@').map_or(body, |(b, _)| b);
            let parts: Vec<&str> = body.split('-').collect();
            let id = match parts.len() {
                1 => parts[0],
                2 => parts[1],
                _ => return None,
            };
            // D-Bus activated session services: app-dbus-:1.2-org.x@0.service
            if id.starts_with(':') || parts.first() == Some(&"dbus") {
                return None;
            }
            return checked(unescape(id));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ual_units() {
        assert_eq!(
            app_id_from_unit("lomiri-app-launch--application-legacy--morph-browser--.service")
                .as_deref(),
            Some("morph-browser")
        );
        assert_eq!(
            app_id_from_unit("lomiri-app-launch--application-legacy--morph-browser--12345.service")
                .as_deref(),
            Some("morph-browser")
        );
        assert_eq!(
            app_id_from_unit(
                "lomiri-app-launch--application-legacy--de.haeckerfelix.Fragments--.service"
            )
            .as_deref(),
            Some("de.haeckerfelix.Fragments")
        );
        assert_eq!(
            app_id_from_unit(r"lomiri-app-launch--application-legacy--a\x2db--.service").as_deref(),
            Some("a-b")
        );
        assert_eq!(
            app_id_from_unit("lomiri-app-launch--application-legacy----.service"),
            None
        );
        // escaped slash or space never yields an id
        assert_eq!(
            app_id_from_unit(r"lomiri-app-launch--application-legacy--a\x2fb--.service"),
            None
        );
    }

    #[test]
    fn xdg_units() {
        assert_eq!(
            app_id_from_unit("app-lomiri-morph\\x2dbrowser-1234.scope").as_deref(),
            Some("morph-browser")
        );
        assert_eq!(
            app_id_from_unit("app-org.example.Chat-77.scope").as_deref(),
            Some("org.example.Chat")
        );
        assert_eq!(
            app_id_from_unit("app-flatpak-org.gnome.Maps-4242.scope").as_deref(),
            Some("org.gnome.Maps")
        );
        assert_eq!(
            app_id_from_unit("app-gnome-firefox@abc.service").as_deref(),
            Some("firefox")
        );
        assert_eq!(
            app_id_from_unit("app-firefox.service").as_deref(),
            Some("firefox")
        );
        assert_eq!(
            app_id_from_unit(r"app-dbus\x2d:1.2\x2dorg.a11y.Bus@0.service"),
            None
        );
        assert_eq!(
            app_id_from_unit("app-dbus-:1.2-org.a11y.Bus@0.service"),
            None
        );
        assert_eq!(app_id_from_unit("app.slice"), None);
        assert_eq!(app_id_from_unit("app-a-b-c-d.scope"), None);
        assert_eq!(app_id_from_unit("fp6-permission-agent.service"), None);
    }
}
