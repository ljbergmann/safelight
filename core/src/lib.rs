//! The API the iPad app's Swift code calls, generated into Swift by UniFFI.
//!
//! Stage 0 of the proof of concept: one call that proves the Rust crates run
//! inside the app and see its sandbox.

uniffi::setup_scaffolding!();

mod ai;
mod alloc;
mod camera_thumbnail;
mod compat;
#[global_allocator]
static ALLOCATOR: alloc::Allocator = alloc::Allocator;

mod compare;
pub use compare::{BeforeAfterCopy, Compare, after_area};
mod controls;
mod coreml;
mod curve;
mod denoise_measure;
pub use curve::CurveState;
mod edit;
mod export;
mod export_extras;
mod files;
mod geometry_upright;
pub use ai::{AiModel, AiModelInfo, AiSelection, ModelInstall, ai_models};
pub use controls::{Control, Section, SliderState, Switch, SwitchState};
pub use export::{ExportChoice, ExportFormat};
pub use files::{FileChange, FolderInfo};
mod guides;
mod import;
mod lens;
mod lensfun;
mod library;
pub use edit::{EditState, Orientation};
pub use import::{ImportCandidate, ImportJob, ImportMode};
mod live;
mod masks;
mod metadata;
mod neural_denoise;
mod neural_sky;
pub use library::{Added, CollectionInfo, Library, LibraryPhoto, SortChoice, sort_orders};
pub use masks::{ComponentInfo, MaskInfo, MaskOperation, NewMask, Stroke};
pub use metadata::{KeywordInfo, MetadataField, PhotoMetadata};
mod point_color;
mod profile;
pub use point_color::{RangeInfo, SwatchInfo};
pub use profile::{ImportSummary, ProfileInfo, import_profiles};
mod render;
mod retouch;
mod snapshots;
pub use snapshots::SnapshotInfo;
mod targeted;
mod tone;
mod transfer;
mod wb;
pub use guides::{CropGuideInfo, crop_guide_lines, crop_guides};
pub use live::{FrameStats, HistogramData, LiveView, MaskOverlay, Zoom};
pub use render::{Backend, DecodeSize, RenderError, Rendering, render_photo};
pub use targeted::TargetKind;
pub use tone::{ParametricCurve, PointCurveMenu};
pub use transfer::{
    CopyGroup, PresetAmountState, PresetInfo, copy_groups, delete_preset, import_presets,
    preset_groups, presets, rename_preset, set_preset_favorite,
};
pub use wb::AutoKind;

/// Where RAWmakase's crates put their files on this device.
#[derive(uniffi::Record)]
pub struct Folders {
    /// The data folder (`RAWMAKASE_DATA_DIR`, else the platform's).
    pub data: String,
    /// This device's own data folder, never synced.
    pub local_data: String,
}

/// The folders RAWmakase's crates chose, as the app sees them.
#[uniffi::export]
pub fn folders() -> Folders {
    Folders {
        data: rawmakase_protocol::paths::data_dir().display().to_string(),
        local_data: rawmakase_model::storage::local_data_dir()
            .display()
            .to_string(),
    }
}

/// Downloads and installs Lensfun's lens profiles, on the user's request
/// only; says what it did. Takes a few seconds: call it off the main thread.
#[uniffi::export]
pub fn install_lensfun() -> Result<String, render::RenderError> {
    lensfun::install()
}

/// Removes Lensfun's lens profiles; edits keep their lens's calibration.
#[uniffi::export]
pub fn remove_lensfun() -> Result<bool, render::RenderError> {
    lensfun::remove()
}

/// The installed Lensfun profiles' date, if any.
#[uniffi::export]
pub fn lensfun_installed() -> Option<String> {
    lensfun::current().map(|i| i.manifest.date())
}

/// A greeting from Rust, to show the bridge works at all.
#[uniffi::export]
pub fn hello() -> String {
    format!(
        "Safelight core {} running on Rust",
        env!("CARGO_PKG_VERSION")
    )
}
