//! Export: photos of the catalog developed to JPEG, TIFF or PNG through
//! RAWmakase's own export job, the one the desktop's Export dialog and batch
//! run (full-size decode, the CPU reference render, metadata and colour
//! profile, written whole or not at all). The app shares the files.

use crate::library::Library;
use crate::render::{RenderError, on_core_thread};
use rawmakase_cancel::Cancel;
use rawmakase_catalog::catalog::PhotoId;
use rawmakase_export::export::job::{self, Photo};
use rawmakase_export::export::{Replace, assemble::Values};
use rawmakase_interop::export_settings::{ExportSettings, Format};
use rawmakase_model::camera_data::Decode;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// 8-bit sRGB JPEG.
    Jpeg,
    /// 16-bit TIFF.
    Tiff,
    Png,
}

/// What the Export sheet chose.
#[derive(uniffi::Record, Clone)]
pub struct ExportChoice {
    pub format: ExportFormat,
    /// JPEG quality, 1–100.
    pub quality: u8,
    /// The long edge in pixels; 0 keeps the full size.
    pub long_edge: u32,
}

impl ExportChoice {
    fn settings(&self) -> ExportSettings {
        ExportSettings {
            format: match self.format {
                ExportFormat::Jpeg => Format::Jpeg,
                ExportFormat::Tiff => Format::Tiff,
                ExportFormat::Png => Format::Png,
            },
            quality: self.quality.clamp(1, 100),
            resize: self.long_edge > 0,
            long_edge: self.long_edge.max(1),
            ..ExportSettings::default()
        }
    }
}

#[uniffi::export]
impl Library {
    /// Exports photos `ids` with `choice` into `folder`, one at a time (each
    /// decodes at full size); returns the files written, in order. A photo
    /// that fails stops the export with its name.
    pub fn export_photos(
        self: Arc<Self>,
        ids: Vec<i64>,
        choice: ExportChoice,
        folder: String,
    ) -> Result<Vec<String>, RenderError> {
        on_core_thread(move || {
            let folder = PathBuf::from(folder);
            std::fs::create_dir_all(&folder).map_err(anyhow::Error::from)?;
            let photos = self.photos()?;
            let settings = choice.settings();
            let mut written = Vec::new();
            for id in ids {
                let Some(photo) = photos.iter().find(|p| p.id == id) else {
                    continue;
                };
                let target = export_one(
                    &self,
                    id,
                    Path::new(&photo.path),
                    photo.rating,
                    &settings,
                    &folder,
                )
                .map_err(|e| anyhow::anyhow!("{}: {e:#}", photo.filename))?;
                written.push(target.display().to_string());
            }
            Ok(written)
        })
    }
}

fn export_one(
    library: &Library,
    id: i64,
    path: &Path,
    rating: i32,
    settings: &ExportSettings,
    folder: &Path,
) -> anyhow::Result<PathBuf> {
    let file = rawmakase_model::storage::Identity::read(path).ok();
    let raw = rawmakase_decode::photo::open(path)?;
    let profiles = rawmakase_decode::camera_look::with_default(
        path,
        &raw.metadata,
        rawmakase_model::camera_profiles::installed(&raw.metadata).0,
        Cancel::NEVER,
    );
    let record = library.catalog().edit_record(PhotoId(id))?;
    let recipe = rawmakase_interop::edits::resolve(
        &record,
        path,
        &raw.metadata,
        &profiles,
        &rawmakase_interop::raw_defaults::DevelopDefaults::default(),
    )?
    .recipe;
    let image = raw.develop(Decode::full(Default::default()), Cancel::NEVER)?;
    let stem = path
        .file_stem()
        .map_or("export".into(), |s| s.to_string_lossy());
    let target = folder.join(format!("{stem}.{}", settings.format.extension()));
    job::run(
        Photo {
            image: Arc::new(image),
            source: path.to_path_buf(),
            recipe,
            values: Values {
                rating,
                ..Values::default()
            },
            watermark: None,
            demosaic: Default::default(),
            cache: library.decode_cache(),
            // The live view's service: a result it finished is reused, and an
            // export of a photo it hasn't denoised waits for its run.
            ai: rawmakase_decode::ai::denoise::AiService(Arc::new(library.ai.denoise().clone())),
            file,
        },
        settings,
        &target,
        Replace::Overwrite,
        Cancel::NEVER,
        |_| {},
    )?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photo_exports_as_a_jpeg_at_the_size_chosen() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(
            dir.path().join("data").display().to_string(),
            dir.path().join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        let chart = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        std::fs::copy(chart, day.join("chart.dng")).unwrap();
        library.add_new_photos().unwrap();
        let id = library.photos().unwrap()[0].id;

        let out = dir.path().join("out");
        let files = library
            .clone()
            .export_photos(
                vec![id],
                ExportChoice {
                    format: ExportFormat::Jpeg,
                    quality: 85,
                    long_edge: 400,
                },
                out.display().to_string(),
            )
            .unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("chart.jpg"), "{}", files[0]);
        let (w, h) = image::image_dimensions(&files[0]).unwrap();
        assert_eq!(w.max(h), 400);
    }

    /// An export with AI Denoise on runs the model through the library's
    /// service. Needs the AI Denoise model installed in this Mac's data
    /// folder and `RAWMAKASE_ORT_LIB` naming an ONNX Runtime library:
    /// `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs the AI Denoise model and RAWMAKASE_ORT_LIB"]
    fn an_export_with_ai_denoise_on_runs_the_model() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(
            dir.path().join("data").display().to_string(),
            dir.path().join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        let chart = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        std::fs::copy(chart, day.join("chart.dng")).unwrap();
        library.add_new_photos().unwrap();
        let photo = &library.photos().unwrap()[0];
        let raw = rawmakase_decode::photo::open(Path::new(&photo.path)).unwrap();
        let profiles = rawmakase_decode::camera_look::with_default(
            Path::new(&photo.path),
            &raw.metadata,
            rawmakase_model::camera_profiles::installed(&raw.metadata).0,
            Cancel::NEVER,
        );
        let mut edit = crate::edit::OpenEdit::load(
            library.clone(),
            photo.id,
            photo.path.clone().into(),
            raw.metadata,
            &profiles,
        )
        .unwrap();
        assert!(edit.switch(crate::controls::Switch::AiDenoise, true));
        edit.save().unwrap();
        drop(edit);

        let files = library
            .clone()
            .export_photos(
                vec![photo.id],
                ExportChoice {
                    format: ExportFormat::Jpeg,
                    quality: 85,
                    long_edge: 400,
                },
                dir.path().join("out").display().to_string(),
            )
            .unwrap();
        assert_eq!(files.len(), 1);
        // The result is cached for the live view to reuse.
        assert!(
            std::fs::read_dir(library.ai.denoise.cache().dir())
                .unwrap()
                .next()
                .is_some()
        );
    }
}
