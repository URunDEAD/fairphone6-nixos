//! fp6-ota: signed over-the-air updates for NixOS on the Fairphone 6.
//! Release side: bin/release.rs (fp6-release). Phone side: bin/updated.rs
//! (fp6-updated: D-Bus service, health check, read-only slot reports).
pub mod bootimg;
pub mod config;
pub mod engine;
pub mod gpt;
pub mod health;
pub mod manifest;
pub mod minisig;
pub mod nixops;
pub mod pairs;
pub mod policy;
pub mod qbootctl;
pub mod state;
pub mod system;
pub mod util;
