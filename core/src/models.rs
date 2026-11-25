//! The model packs Safelight installs: Select Subject's, Select Sky's and AI
//! Denoise's. RAWmakase's desktop app installs the selection models through its
//! AI Masks extension and AI Denoise's through `rawmakase_denoise`; both use
//! `rawmakase_model_install`, which this uses too, with the same folders and
//! receipts, so a pack installed by an earlier Safelight stays installed.
use rawmakase_inference::ModelFile;
use std::path::{Path, PathBuf};

pub(crate) use rawmakase_model_install::{InstallError, Progress};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Pack {
    #[default]
    Subject,
    Sky,
    /// AI Denoise's model, with its `LICENSE` and `NOTICE`.
    Denoise,
}

impl Pack {
    fn files(self) -> Vec<ModelFile> {
        match self {
            Self::Subject => rawmakase_selection::manifest::all_files(),
            Self::Sky => rawmakase_selection::manifest::sky_files(),
            Self::Denoise => rawmakase_inference::manifest::denoise_files(),
        }
    }

    pub(crate) fn bytes(self) -> u64 {
        self.files().iter().map(|f| f.size_bytes).sum()
    }

    /// Where it is kept: the local data folder's `models`, in a folder named for
    /// the model and its contract version, as RAWmakase keeps it.
    pub(crate) fn installed_dir(self) -> PathBuf {
        let (id, version) = match self {
            Self::Subject => (
                rawmakase_selection::SUBJECT.id,
                rawmakase_selection::SUBJECT.version,
            ),
            Self::Sky => (
                rawmakase_selection::SKY.id,
                rawmakase_selection::SKY.version,
            ),
            Self::Denoise => return rawmakase_denoise::model_install::dir(),
        };
        rawmakase_model_install::models_dir().join(format!("{id}-v{version}"))
    }

    /// The megabytes the drawer quotes.
    pub(crate) fn megabytes(self) -> u64 {
        self.bytes().div_ceil(1_000_000)
    }

    /// Whether every file is in place and verified.
    pub(crate) fn is_installed(self) -> bool {
        rawmakase_model_install::is_installed(&self.installed_dir(), &self.files())
    }
}

/// Fetches or copies `pack`'s files into its folder and checks each against its
/// SHA-256; see [`rawmakase_model_install::install`].
pub(crate) fn install(
    pack: Pack,
    import: Option<&Path>,
    progress: &Progress,
) -> Result<(), InstallError> {
    rawmakase_model_install::install(&pack.installed_dir(), &pack.files(), import, progress)
}

/// Where the app looks for an ONNX Runtime library it did not come with.
pub(crate) fn runtime_dir() -> PathBuf {
    rawmakase_model_install::runtime_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folders RAWmakase 0.4's installer used, which installs on iPads
    /// already have.
    #[test]
    fn packs_stay_where_earlier_versions_installed_them() {
        let models = rawmakase_model::storage::local_data_dir().join("models");
        let named = |id: &str, v: u32| models.join(format!("{id}-v{v}"));
        assert_eq!(
            Pack::Subject.installed_dir(),
            named(
                rawmakase_selection::SUBJECT.id,
                rawmakase_selection::SUBJECT.version
            )
        );
        assert_eq!(
            Pack::Sky.installed_dir(),
            named(
                rawmakase_selection::SKY.id,
                rawmakase_selection::SKY.version
            )
        );
        assert_eq!(
            Pack::Denoise.installed_dir(),
            named(
                rawmakase_inference::RAWNIND.id,
                rawmakase_inference::RAWNIND.version
            )
        );
    }
}
