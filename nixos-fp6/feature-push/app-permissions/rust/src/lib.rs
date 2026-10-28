// SPDX-License-Identifier: GPL-3.0-or-later
//! FP6 runtime app permissions: Android-style prompts (Allow / Allow only
//! while using / Deny, remembered per app, revocable in Settings) for USB
//! devices, NFC, Bluetooth, contacts, calendar, call log & messages, files
//! and sensors, plus the per-app network permission (enforced by netpolicy).
//! See results/feature-push-20261024/app-permissions/README.md.

pub mod appid;
pub mod broker;
pub mod device;
pub mod model;
pub mod policy;
pub mod render;

/// System-bus names.
pub const BUS_NAME: &str = "org.fairphone.Permissions1";
pub const PATH: &str = "/org/fairphone/Permissions1";
pub const AGENT_IFACE: &str = "org.fairphone.PermissionAgent1";
pub const AGENT_PATH: &str = "/org/fairphone/PermissionAgent";
/// Session-bus action group of the agent (Lomiri prompt + Settings pages).
pub const SESSION_NAME: &str = "com.fairphone6.Permissions";
pub const SESSION_PATH: &str = "/com/fairphone6/Permissions";
