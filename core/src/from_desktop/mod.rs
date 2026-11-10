//! Safelight's copies of code that RAWmakase keeps in its desktop app, whose
//! crate doesn't build for iOS. Each is upstream's logic as it stood at
//! `ea3d0634`, taken out of its window code, with methods on RAWmakase's types
//! written as functions. Keep them in step with upstream: docs/FORK.md lists
//! where each came from and how to check it.
//!
//! What Safelight doesn't call stays in, so a copy compares easily with its
//! original.
#![allow(dead_code)]

pub mod bulk_import;
pub mod crop_guides;
pub mod geometry;
pub mod grading;
pub mod masks;
pub mod photo_info;
pub mod point_color;
pub mod retouch;
