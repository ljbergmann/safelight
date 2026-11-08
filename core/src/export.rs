//! Export: photos of the catalog developed to JPEG, TIFF, PNG or JPEG XL
//! through RAWmakase's own export batch, the one the desktop's Export dialog
//! runs: its file naming, colour spaces, metadata choices and the Simple
//! Copyright Watermark, each photo with the catalog's edit and metadata. The
//! app shares the files.
//!
//! The choices are kept as the desktop's [`ExportSettings`] (its
//! `export.json` format): the last export's, and presets saved by name in
//! `export_presets.json` in the library's data folder. A preset written by a
//! later release keeps what this one can't read.

use crate::library::Library;
use crate::render::{RenderError, on_core_thread};
use rawmakase_cancel::Cancel;
use rawmakase_catalog::catalog::PhotoId;
use rawmakase_export::export::batch::{self, Batch, BatchPhoto, Outcome};
use rawmakase_interop::export_settings::{
    Destination, Existing, ExportSettings, Format, Include, NameContext, Naming,
};
use rawmakase_model::color::output::OutputSpace;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// 8-bit JPEG.
    Jpeg,
    /// 16-bit TIFF.
    Tiff,
    Png,
    /// JPEG XL, lossy at the quality chosen.
    Jxl,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportColorSpace {
    Srgb,
    AdobeRgb,
    ProPhoto,
}

/// Lightroom's Rename To presets.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportNaming {
    Filename,
    FilenameSequence,
    FilenameText,
    CustomName,
    CustomNameSequence,
    /// "Text (1 of 3)".
    CustomNameOf,
    /// "20260504-DSC0001", from the capture date.
    DateFilename,
}

/// Lightroom's Metadata: Include.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportMetadata {
    CopyrightOnly,
    CopyrightAndContact,
    AllExceptCameraAndCameraRaw,
    AllExceptCameraRaw,
    All,
}

/// What the Export sheet chose.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ExportChoice {
    pub format: ExportFormat,
    /// JPEG and JPEG XL quality, 1–100.
    pub quality: u8,
    /// The long edge in pixels; 0 keeps the full size.
    pub long_edge: u32,
    pub color_space: ExportColorSpace,
    pub naming: ExportNaming,
    /// Rename To's Custom Text, and where Sequence starts.
    pub custom_text: String,
    pub start_number: u32,
    pub metadata: ExportMetadata,
    /// Lightroom's Remove Location Info.
    pub remove_location: bool,
    /// The Simple Copyright Watermark: each photo's copyright over it.
    pub watermark: bool,
}

impl Default for ExportChoice {
    fn default() -> Self {
        Self::from_settings(&ExportSettings::default())
    }
}

/// An export preset: a name and its choices.
#[derive(uniffi::Record, Clone, Debug)]
pub struct ExportPreset {
    pub name: String,
    pub choice: ExportChoice,
}

/// What an export wrote, and what it has to say about the photos it
/// couldn't export or named without something the name needed.
#[derive(uniffi::Record, Clone, Debug)]
pub struct ExportResult {
    pub files: Vec<String>,
    pub problems: Vec<String>,
}

impl ExportChoice {
    /// The choices as the desktop's settings: `base`'s, so what a preset
    /// or a later release set and this sheet doesn't show is kept.
    fn apply(&self, base: &ExportSettings) -> ExportSettings {
        let mut s = base.clone();
        s.choose_format(match self.format {
            ExportFormat::Jpeg => Format::Jpeg,
            ExportFormat::Tiff => Format::Tiff,
            ExportFormat::Png => Format::Png,
            ExportFormat::Jxl => Format::Jxl,
        });
        s.choose_color_space(match self.color_space {
            ExportColorSpace::Srgb => OutputSpace::Srgb,
            ExportColorSpace::AdobeRgb => OutputSpace::AdobeRgb,
            ExportColorSpace::ProPhoto => OutputSpace::ProPhoto,
        });
        if s.naming() != naming(self.naming) || s.custom_tokens().is_some() {
            s.choose_naming(naming(self.naming));
        }
        s.custom_text = self.custom_text.clone();
        s.start_number = self.start_number.max(1);
        s.quality = self.quality.clamp(1, 100);
        s.jxl_lossy = true;
        s.resize = self.long_edge > 0;
        if self.long_edge > 0 {
            s.long_edge = self.long_edge;
        }
        s.include = match self.metadata {
            ExportMetadata::CopyrightOnly => Include::CopyrightOnly,
            ExportMetadata::CopyrightAndContact => Include::CopyrightAndContact,
            ExportMetadata::AllExceptCameraAndCameraRaw => Include::AllExceptCameraAndCameraRaw,
            ExportMetadata::AllExceptCameraRaw => Include::AllExceptCameraRaw,
            ExportMetadata::All => Include::All,
        };
        s.remove_location = self.remove_location;
        s.watermark = self.watermark;
        if self.watermark {
            s.watermark_name = rawmakase_export::watermark::SIMPLE_COPYRIGHT.into();
        }
        s
    }

    fn from_settings(s: &ExportSettings) -> Self {
        Self {
            format: match s.format {
                Format::Jpeg => ExportFormat::Jpeg,
                Format::Tiff => ExportFormat::Tiff,
                Format::Png => ExportFormat::Png,
                Format::Jxl => ExportFormat::Jxl,
            },
            quality: s.quality,
            long_edge: if s.resize { s.long_edge } else { 0 },
            color_space: match s.color_space {
                OutputSpace::Srgb => ExportColorSpace::Srgb,
                OutputSpace::AdobeRgb => ExportColorSpace::AdobeRgb,
                OutputSpace::ProPhoto => ExportColorSpace::ProPhoto,
            },
            naming: match s.naming() {
                Naming::Filename => ExportNaming::Filename,
                Naming::FilenameText => ExportNaming::FilenameText,
                Naming::FilenameSequence => ExportNaming::FilenameSequence,
                Naming::CustomName => ExportNaming::CustomName,
                Naming::CustomNameSequence => ExportNaming::CustomNameSequence,
                Naming::CustomNameOf => ExportNaming::CustomNameOf,
                Naming::DateFilename => ExportNaming::DateFilename,
            },
            custom_text: s.custom_text.clone(),
            start_number: s.start_number,
            // The desktop's Custom (its four switches) is closest to All here.
            metadata: match s.include {
                Include::CopyrightOnly => ExportMetadata::CopyrightOnly,
                Include::CopyrightAndContact => ExportMetadata::CopyrightAndContact,
                Include::AllExceptCameraAndCameraRaw => ExportMetadata::AllExceptCameraAndCameraRaw,
                Include::AllExceptCameraRaw => ExportMetadata::AllExceptCameraRaw,
                Include::All | Include::Custom => ExportMetadata::All,
            },
            remove_location: s.remove_location,
            // Only the Simple Copyright Watermark: the desktop's own presets
            // aren't on the iPad.
            watermark: s.watermark
                && s.watermark_name == rawmakase_export::watermark::SIMPLE_COPYRIGHT,
        }
    }
}

fn naming(n: ExportNaming) -> Naming {
    match n {
        ExportNaming::Filename => Naming::Filename,
        ExportNaming::FilenameSequence => Naming::FilenameSequence,
        ExportNaming::FilenameText => Naming::FilenameText,
        ExportNaming::CustomName => Naming::CustomName,
        ExportNaming::CustomNameSequence => Naming::CustomNameSequence,
        ExportNaming::CustomNameOf => Naming::CustomNameOf,
        ExportNaming::DateFilename => Naming::DateFilename,
    }
}

/// The saved presets, by name, each as the desktop's settings; a preset
/// this release can't read is kept as it was.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PresetFile {
    #[serde(default)]
    presets: Vec<serde_json::Value>,
    #[serde(flatten)]
    later: serde_json::Map<String, serde_json::Value>,
}

impl Library {
    fn last_export_path(&self) -> PathBuf {
        self.data_dir().join("export.json")
    }

    fn presets_path(&self) -> PathBuf {
        self.data_dir().join("export_presets.json")
    }

    /// The last export's settings, or the defaults.
    fn last_export(&self) -> ExportSettings {
        ExportSettings::load_file(&self.last_export_path()).unwrap_or_default()
    }

    fn preset_file(&self) -> anyhow::Result<PresetFile> {
        match std::fs::read(self.presets_path()) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PresetFile::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn save_presets(&self, file: &PresetFile) -> anyhow::Result<()> {
        rawmakase_model::storage::atomic_json(&self.presets_path(), file)
    }
}

/// A preset's name in its file.
fn preset_name(v: &serde_json::Value) -> Option<&str> {
    v.get("name")?.as_str()
}

#[uniffi::export]
impl Library {
    /// What the last export chose, for the Export sheet to start from.
    pub fn last_export_choice(&self) -> ExportChoice {
        ExportChoice::from_settings(&self.last_export())
    }

    /// The saved export presets, by name.
    pub fn export_presets(&self) -> Vec<ExportPreset> {
        let Ok(file) = self.preset_file() else {
            return vec![];
        };
        let mut presets: Vec<ExportPreset> = file
            .presets
            .iter()
            .filter_map(|v| {
                let name = preset_name(v)?.to_string();
                let settings = settings_of(v)?;
                Some(ExportPreset {
                    name,
                    choice: ExportChoice::from_settings(&settings),
                })
            })
            .collect();
        presets.sort_by_key(|p| p.name.to_lowercase());
        presets
    }

    /// Saves `choice` as preset `name`, replacing one of that name.
    pub fn save_export_preset(
        &self,
        name: String,
        choice: ExportChoice,
    ) -> Result<(), RenderError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(anyhow::anyhow!("A preset needs a name").into());
        }
        let mut file = self.preset_file()?;
        let old = file
            .presets
            .iter()
            .position(|v| preset_name(v) == Some(&name));
        let base = old
            .and_then(|i| settings_of(&file.presets[i]))
            .unwrap_or_else(|| self.last_export());
        let mut saved = serde_json::Map::new();
        saved.insert("name".into(), name.into());
        saved.insert("settings".into(), settings_json(&choice.apply(&base))?);
        match old {
            Some(i) => file.presets[i] = saved.into(),
            None => file.presets.push(saved.into()),
        }
        Ok(self.save_presets(&file)?)
    }

    pub fn delete_export_preset(&self, name: String) -> Result<(), RenderError> {
        let mut file = self.preset_file()?;
        file.presets.retain(|v| preset_name(v) != Some(&name));
        Ok(self.save_presets(&file)?)
    }

    /// How `choice` names photo `id`, the first of `total` exported, as the
    /// sheet's Example shows it: from what the catalog knows of it, as the
    /// export names it.
    pub fn export_example_name(&self, choice: ExportChoice, id: i64, total: u32) -> String {
        let settings = choice.apply(&self.last_export());
        let photos = self.photos().unwrap_or_default();
        let Some(photo) = photos.iter().find(|p| p.id == id) else {
            return String::new();
        };
        let fields = self
            .catalog()
            .name_fields(&[PhotoId(id)])
            .ok()
            .and_then(|f| f.into_iter().next())
            .unwrap_or_default();
        let at = NameContext {
            index: 0,
            total: total.max(1) as usize,
            fields: &fields,
        };
        settings.file_name_for(Path::new(&photo.path), &at).0
    }

    /// Exports photos `ids` with `choice` into `folder` through the
    /// desktop's export batch, one photo at a time (each decodes at full
    /// size), and remembers the choice for next time. Returns the files
    /// written, in order, and what went wrong with the others.
    pub fn export_photos(
        self: Arc<Self>,
        ids: Vec<i64>,
        choice: ExportChoice,
        folder: String,
    ) -> Result<ExportResult, RenderError> {
        on_core_thread(move || {
            let folder = PathBuf::from(folder);
            std::fs::create_dir_all(&folder).map_err(anyhow::Error::from)?;
            let last = self.last_export();
            let mut settings = choice.apply(&last);
            // Remembered as chosen, without this export's folder.
            let _ = settings.save_file(&self.last_export_path());
            settings.destination = Destination::Folder;
            settings.folder = Some(folder);
            settings.subfolder = false;
            settings.existing = Existing::Unique;

            Ok(self.run_batch(&ids, settings)?)
        })
    }

    /// Photo `id` as edited, `edge` pixels on its long side, as a JPEG file
    /// for Reference View: rendered as an export would, then kept in the
    /// caches until the edit changes. Call it off the main thread.
    pub fn reference_image(self: Arc<Self>, id: i64, edge: u32) -> Result<String, RenderError> {
        on_core_thread(move || {
            let edited = self
                .catalog()
                .edit_times()?
                .remove(&PhotoId(id))
                .unwrap_or_default();
            let dir = self.caches_dir().join("Reference");
            let key = dir.join(format!(
                "{id}-{:016x}-{edge}.jpg",
                crate::library::fnv(edited.as_bytes())
            ));
            if key.exists() {
                return Ok(key.display().to_string());
            }
            let work = dir.join(format!("{id}-rendering"));
            let _ = std::fs::remove_dir_all(&work);
            std::fs::create_dir_all(&work).map_err(anyhow::Error::from)?;
            let mut settings = ExportSettings::default();
            settings.choose_format(Format::Jpeg);
            settings.quality = 90;
            settings.resize = true;
            settings.long_edge = edge.max(64);
            settings.include = Include::CopyrightOnly;
            settings.destination = Destination::Folder;
            settings.folder = Some(work.clone());
            settings.existing = Existing::Overwrite;
            let result = self.run_batch(&[id], settings)?;
            let file = result
                .files
                .first()
                .ok_or_else(|| anyhow::anyhow!("{}", result.problems.join("; ")))?;
            std::fs::rename(file, &key).map_err(anyhow::Error::from)?;
            let _ = std::fs::remove_dir_all(&work);
            Ok(key.display().to_string())
        })
    }
}

impl Library {
    /// Exports photos `ids` with `settings` (its folder set) through the
    /// desktop's export batch: the files written and what went wrong.
    fn run_batch(&self, ids: &[i64], settings: ExportSettings) -> anyhow::Result<ExportResult> {
        let photos = self.photos()?;
        let chosen: Vec<_> = ids
            .iter()
            .filter_map(|id| photos.iter().find(|p| p.id == *id))
            .collect();
        let photo_ids: Vec<PhotoId> = chosen.iter().map(|p| PhotoId(p.id)).collect();
        let (records, fields) = {
            let catalog = self.catalog();
            (
                catalog.photo_records(&photo_ids)?,
                catalog.name_fields(&photo_ids)?,
            )
        };
        let total = chosen.len();
        let batch_photos: Vec<BatchPhoto> = chosen
            .iter()
            .zip(records)
            .zip(fields)
            .enumerate()
            .map(|(i, ((p, record), naming))| {
                let name = if p.is_copy {
                    format!("{} ({})", p.filename, p.copy_name)
                } else {
                    p.filename.clone()
                };
                let mut photo =
                    BatchPhoto::from_record(record, PathBuf::from(&p.path), name, naming);
                photo.place = Some((i, total));
                photo
            })
            .collect();
        let watermark = if settings.watermark {
            Some(
                rawmakase_export::watermark::named(&settings.watermark_name).ok_or_else(|| {
                    anyhow::anyhow!("Watermark not found: {}", settings.watermark_name)
                })?,
            )
        } else {
            None
        };
        let plan =
            batch::plan(&batch_photos, &settings, None).map_err(|e| anyhow::anyhow!("{e}"))?;
        let names: Vec<String> = batch_photos.iter().map(|p| p.name.clone()).collect();
        let batch = Batch {
            photos: batch_photos,
            plan,
            settings,
            defaults: Arc::new(Default::default()),
            watermark,
            demosaic: Default::default(),
            cache: self.decode_cache(),
            // The live view's service: a result it finished is reused, and
            // an export of a photo it hasn't denoised waits for its run.
            ai: rawmakase_decode::ai::denoise::AiService(Arc::new(self.ai.denoise().clone())),
        };
        let mut result = ExportResult {
            files: vec![],
            problems: vec![],
        };
        for (name, outcome) in names.iter().zip(batch::run(&batch, Cancel::NEVER, |_| {})) {
            match outcome {
                Outcome::Exported { path, notes } => {
                    result.files.push(path.display().to_string());
                    result
                        .problems
                        .extend(notes.into_iter().map(|n| format!("{name}: {n}")));
                }
                Outcome::Skipped(why) | Outcome::Failed(why) => {
                    result.problems.push(format!("{name}: {why}"))
                }
                Outcome::Cancelled | Outcome::NotStarted => {}
            }
        }
        if result.files.is_empty()
            && let Some(first) = result.problems.first()
        {
            return Err(anyhow::anyhow!("{first}"));
        }
        Ok(result)
    }
}

/// A preset's settings, read as the desktop reads `export.json`.
fn settings_of(v: &serde_json::Value) -> Option<ExportSettings> {
    ExportSettings::from_value(v.get("settings")?.clone())
}

/// Settings as the desktop writes them, later releases' choices kept.
fn settings_json(s: &ExportSettings) -> anyhow::Result<serde_json::Value> {
    s.to_value()
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
                    ..ExportChoice::default()
                },
                out.display().to_string(),
            )
            .unwrap()
            .files;
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("chart.jpg"), "{}", files[0]);
        let (w, h) = image::image_dimensions(&files[0]).unwrap();
        assert_eq!(w.max(h), 400);
    }

    /// A library with the chart filed twice, as two photos.
    fn two_charts() -> (tempfile::TempDir, Arc<Library>, Vec<i64>) {
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
        std::fs::copy(&chart, day.join("a.dng")).unwrap();
        std::fs::copy(&chart, day.join("b.dng")).unwrap();
        library.add_new_photos().unwrap();
        let mut photos = library.photos().unwrap();
        photos.sort_by(|a, b| a.filename.cmp(&b.filename));
        let ids = photos.iter().map(|p| p.id).collect();
        (dir, library, ids)
    }

    #[test]
    fn photos_are_named_by_the_template_and_the_choice_is_remembered() {
        let (dir, library, ids) = two_charts();
        let choice = ExportChoice {
            format: ExportFormat::Png,
            long_edge: 200,
            color_space: ExportColorSpace::AdobeRgb,
            naming: ExportNaming::CustomNameSequence,
            custom_text: "Trip".into(),
            start_number: 7,
            metadata: ExportMetadata::CopyrightOnly,
            watermark: true,
            ..ExportChoice::default()
        };
        assert_eq!(
            library.export_example_name(choice.clone(), ids[0], 2),
            "Trip-7.png"
        );
        let out = dir.path().join("out");
        let result = library
            .clone()
            .export_photos(ids.clone(), choice.clone(), out.display().to_string())
            .unwrap();
        let names: Vec<String> = result
            .files
            .iter()
            .map(|f| {
                Path::new(f)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["Trip-7.png", "Trip-8.png"], "{:?}", result.problems);
        assert_eq!(library.last_export_choice(), choice, "remembered");

        // Again into the same folder: new names, nothing written over.
        let again = library
            .clone()
            .export_photos(ids, choice, out.display().to_string())
            .unwrap();
        assert_eq!(again.files.len(), 2);
        assert!(again.files.iter().all(|f| !result.files.contains(f)));
    }

    #[test]
    fn a_reference_image_is_rendered_once_and_leaves_the_last_export_alone() {
        let (_dir, library, ids) = two_charts();
        let first = library.clone().reference_image(ids[0], 300).unwrap();
        let (w, h) = image::image_dimensions(&first).unwrap();
        assert_eq!(w.max(h), 300);
        let made = std::fs::metadata(&first).unwrap().modified().unwrap();
        let again = library.clone().reference_image(ids[0], 300).unwrap();
        assert_eq!(again, first);
        assert_eq!(
            std::fs::metadata(&again).unwrap().modified().unwrap(),
            made,
            "from the cache"
        );
        assert_ne!(library.clone().reference_image(ids[1], 300).unwrap(), first);
        assert_eq!(library.last_export_choice(), ExportChoice::default());
    }

    #[test]
    fn presets_are_saved_replaced_and_deleted_keeping_what_they_dont_show() {
        let (_dir, library, _) = two_charts();
        assert!(library.export_presets().is_empty());
        let web = ExportChoice {
            long_edge: 2048,
            quality: 80,
            ..ExportChoice::default()
        };
        library
            .save_export_preset("Web".into(), web.clone())
            .unwrap();
        library
            .save_export_preset(
                "Archive".into(),
                ExportChoice {
                    format: ExportFormat::Tiff,
                    color_space: ExportColorSpace::ProPhoto,
                    ..ExportChoice::default()
                },
            )
            .unwrap();
        let names: Vec<_> = library
            .export_presets()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Archive", "Web"]);
        assert_eq!(library.export_presets()[1].choice, web);

        // What the desktop set and the sheet doesn't show survives a save.
        let path = library.presets_path();
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        file["presets"][0]["settings"]["ppi"] = 300.into();
        file["presets"][0]["settings"]["from_later"] = "kept".into();
        file["from_later"] = true.into();
        std::fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
        library
            .save_export_preset(
                "Web".into(),
                ExportChoice {
                    quality: 70,
                    ..web.clone()
                },
            )
            .unwrap();
        let file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let saved = &file["presets"][0]["settings"];
        assert_eq!(
            (saved["ppi"].as_u64(), saved["quality"].as_u64()),
            (Some(300), Some(70))
        );
        assert_eq!(saved["from_later"], "kept");
        assert_eq!(file["from_later"], true);

        library.delete_export_preset("Archive".into()).unwrap();
        assert_eq!(library.export_presets().len(), 1);
        assert!(library.save_export_preset("  ".into(), web).is_err());
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
                    ..ExportChoice::default()
                },
                dir.path().join("out").display().to_string(),
            )
            .unwrap()
            .files;
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
