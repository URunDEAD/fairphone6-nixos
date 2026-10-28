//! fp6-assistant: an on-device assistant for the Fairphone 6 (Lomiri).
//! Chat with a local (on-device) or remote LLM that can use the phone's
//! apps through tools, each gated by per-resource permissions, per-action
//! confirmations, rate limits and an activity log.
//! Design: results/feature-push-20261024/assistant/README.md.

pub mod agent;
pub mod compact;
pub mod dbusutil;
pub mod keyring;
pub mod llm;
pub mod permissions;
pub mod prompts;
pub mod service;
pub mod settings;
pub mod springboard;
pub mod store;
pub mod tools;
