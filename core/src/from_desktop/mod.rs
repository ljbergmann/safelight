//! Safelight's copies of code that RAWmakase keeps in its desktop app, whose
//! crate doesn't build for iOS. Each is upstream's logic as it stood at
//! `ea3d0634`, taken out of its window code, with methods on RAWmakase's types
//! written as functions. Keep them in step with upstream: docs/FORK.md lists
//! where each came from and how to check it.
//!
//! [`ai_denoise`], [`model_install`] and [`selection`] are the fork's former
//! `rawmakase-ai` crate: upstream's `src/ai_denoise/`, `src/model_install/` and
//! what a selection records, with Safelight's own Neural Engine changes on top
//! (the provider a measurement chooses, an app's own model loader, and falling
//! back to the GPU before the CPU).
//!
//! What Safelight doesn't call stays in, so a copy compares easily with its
//! original.
#![allow(dead_code)]

pub mod ai_denoise;
pub mod bulk_import;
pub mod crop_guides;
pub mod geometry;
pub mod grading;
pub mod masks;
pub mod model_install;
pub mod photo_info;
pub mod point_color;
pub mod retouch;
pub mod selection;
