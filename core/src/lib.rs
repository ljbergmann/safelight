//! The API the iPad app's Swift code calls, generated into Swift by UniFFI.
//!
//! Stage 0 of the proof of concept: one call that proves the Rust crates run
//! inside the app and see its sandbox.

uniffi::setup_scaffolding!();

mod ai;
mod alloc;
#[global_allocator]
static ALLOCATOR: alloc::Allocator = alloc::Allocator;

mod compare;
pub use compare::{BeforeAfterCopy, Compare, after_area};
mod controls;
mod curve;
pub use curve::CurveState;
mod edit;
mod export;
pub use ai::{AiModel, AiModelInfo, AiSelection, ModelInstall, ai_models};
pub use controls::{Control, Section, SliderState, Switch, SwitchState};
pub use export::{ExportChoice, ExportFormat};
mod import;
mod library;
pub use edit::{EditState, Orientation};
pub use import::{ImportCandidate, ImportJob, ImportMode};
mod live;
mod masks;
pub use library::{Added, CollectionInfo, Library, LibraryPhoto};
pub use masks::{ComponentInfo, MaskInfo, MaskOperation, NewMask, Stroke};
mod profile;
pub use profile::ProfileInfo;
mod render;
mod snapshots;
pub use snapshots::SnapshotInfo;
mod transfer;
mod wb;
pub use live::{FrameStats, HistogramData, LiveView, MaskOverlay, Zoom};
pub use render::{Backend, DecodeSize, RenderError, Rendering, render_photo};
pub use transfer::{
    CopyGroup, PresetAmountState, PresetInfo, copy_groups, delete_preset, import_presets,
    preset_groups, presets, rename_preset,
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

/// A greeting from Rust, to show the bridge works at all.
#[uniffi::export]
pub fn hello() -> String {
    format!(
        "RAWmakase core {} running on Rust",
        env!("CARGO_PKG_VERSION")
    )
}
