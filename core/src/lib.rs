//! The API the iPad app's Swift code calls, generated into Swift by UniFFI.
//!
//! Stage 0 of the proof of concept: one call that proves the Rust crates run
//! inside the app and see its sandbox.

uniffi::setup_scaffolding!();

mod alloc;
#[global_allocator]
static ALLOCATOR: alloc::Allocator = alloc::Allocator;

mod controls;
mod edit;
mod export;
pub use controls::{Control, Section, SliderState, Switch, SwitchState};
pub use export::{ExportChoice, ExportFormat};
mod library;
pub use edit::{EditState, Orientation};
mod live;
pub use library::{Added, CollectionInfo, Library, LibraryPhoto};
mod render;
mod transfer;
pub use live::{FrameStats, LiveView, Zoom};
pub use render::{Backend, DecodeSize, RenderError, Rendering, render_photo};
pub use transfer::{CopyGroup, PresetInfo, copy_groups, import_presets, presets};

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
