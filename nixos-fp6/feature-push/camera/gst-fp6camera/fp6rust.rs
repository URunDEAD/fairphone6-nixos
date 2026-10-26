// SPDX-License-Identifier: LGPL-2.1-or-later
//! Crate root of the one Rust static library linked into libgstfp6camera.so
//! (libfp6vision.a). Every Rust part of the element lives here as a module:
//! two Rust staticlibs in one shared object would each carry their own copy
//! of std and clash at link time.
//!   vision      panorama / document scan (camera Phase 2)
//!   fastlaunch  stored auto-exposure state (camera fast launch)

#[path = "vision/lib.rs"]
pub mod vision;

#[path = "fastlaunch/lib.rs"]
pub mod fastlaunch;
