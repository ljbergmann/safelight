//! A photo's descriptive metadata and keywords, as the desktop's Metadata and
//! Keywording panels keep them in the catalog, and its camera info.
//!
//! The text the catalog keeps overrides the file's own; a field emptied here
//! is cleared, so the file's value stays out of an export too. Keywords are
//! hierarchical, written "Parent > Child", and made where missing.

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_catalog::catalog::PhotoId;
use rawmakase_model::metadata::{Descriptive, TextField, Value};

/// The fields the Info sheet edits.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataField {
    Title,
    Caption,
    Creator,
    Copyright,
}

/// A keyword on a photo.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct KeywordInfo {
    pub id: i64,
    /// Its path, top first, as "Places > Berlin".
    pub path: String,
}

/// What the Info sheet shows of a photo.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PhotoMetadata {
    /// The catalog's text, empty where it has none.
    pub title: String,
    pub caption: String,
    pub creator: String,
    pub copyright: String,
    pub keywords: Vec<KeywordInfo>,
    /// Camera, lens and settings, as Lightroom's Metadata panel words them.
    pub camera: Vec<String>,
}

/// The separator of a keyword's levels, as Lightroom writes them.
const LEVELS: &str = ">";

fn text(value: Option<&Value<rawmakase_model::metadata::LangAlt>>) -> String {
    match value {
        Some(Value::Set(langs)) => langs.default_text().unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

fn creator(d: &Descriptive) -> String {
    match &d.creator {
        Some(Value::Set(names)) => names.join("; "),
        _ => String::new(),
    }
}

#[uniffi::export]
impl Library {
    /// Photo `id`'s metadata and keywords, and its camera info, read from
    /// the file once and kept in the catalog.
    pub fn metadata(&self, id: i64) -> Result<PhotoMetadata, RenderError> {
        let photo = PhotoId(id);
        let (descriptive, keywords, info, path) = {
            let catalog = self.catalog();
            let path = catalog
                .photos()?
                .into_iter()
                .find(|p| p.id == photo)
                .map(|p| p.path)
                .ok_or_else(|| RenderError::Failed {
                    message: format!("No photo {id} in the catalog"),
                })?;
            (
                catalog.descriptive(photo)?,
                catalog.keywords(photo)?,
                catalog.photo_info(photo)?,
                path,
            )
        };
        let info = match info {
            // An empty record is read again: an early build kept one for
            // RAWs it read the EXIF of alone.
            Some(info) if info != rawmakase_model::metadata::PhotoInfo::default() => Some(info),
            // Read as the desktop reads it: a RAW's through its decoder. A
            // file that can't be read now is tried again next time. On the
            // core's thread: opening a RAW needs more stack than Swift's.
            _ => match crate::render::on_core_thread({
                let path = path.clone();
                move || Ok(rawmakase_decode::photo_info::read_info(&path))
            })? {
                Some(read) => {
                    // A failed write only means it is read again next time.
                    let _ = self.catalog().fill_photo_info(&[(photo, read.clone())]);
                    read
                }
                None => None,
            },
        };
        let camera = info
            .map(|i| {
                [
                    i.camera.clone(),
                    i.lens.clone(),
                    i.exposure_text(),
                    i.focal_text(),
                    i.iso_text(),
                    i.dimensions_text(),
                ]
                .into_iter()
                .flatten()
                .collect()
            })
            .unwrap_or_default();
        Ok(PhotoMetadata {
            title: text(descriptive.text(TextField::Title)),
            caption: text(descriptive.text(TextField::Caption)),
            creator: creator(&descriptive),
            copyright: text(descriptive.text(TextField::Copyright)),
            keywords: keywords
                .into_iter()
                .map(|k| KeywordInfo {
                    id: k.id,
                    path: k.path.join(&format!(" {LEVELS} ")),
                })
                .collect(),
            camera,
        })
    }

    /// Sets `field` of photos `ids` to `text`; empty clears it. Several
    /// creators are separated by ";". One transaction.
    pub fn set_metadata_text(
        &self,
        ids: Vec<i64>,
        field: MetadataField,
        text: String,
    ) -> Result<(), RenderError> {
        let ids: Vec<PhotoId> = ids.into_iter().map(PhotoId).collect();
        let text = text.trim();
        let mut catalog = self.catalog();
        match field {
            MetadataField::Creator => {
                let names: Vec<String> = text.split(';').map(str::to_string).collect();
                catalog.set_creators(&ids, &names)?;
            }
            MetadataField::Title => catalog.set_text(&ids, TextField::Title, text)?,
            MetadataField::Caption => catalog.set_text(&ids, TextField::Caption, text)?,
            MetadataField::Copyright => catalog.set_text(&ids, TextField::Copyright, text)?,
        }
        Ok(())
    }

    /// Adds `keywords` to photos `ids`: several separated by commas, each
    /// "Parent > Child" for one inside another, made where missing.
    pub fn add_keywords(&self, ids: Vec<i64>, keywords: String) -> Result<(), RenderError> {
        let ids: Vec<PhotoId> = ids.into_iter().map(PhotoId).collect();
        let paths: Vec<Vec<String>> = keywords
            .split(',')
            .map(|k| {
                k.split(LEVELS)
                    .map(|level| level.trim().to_string())
                    .filter(|level| !level.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|path| !path.is_empty())
            .collect();
        self.catalog().add_keywords(&ids, &paths)?;
        Ok(())
    }

    /// Takes keyword `keyword` off photos `ids`; the keyword itself stays.
    pub fn remove_keyword(&self, ids: Vec<i64>, keyword: i64) -> Result<(), RenderError> {
        let ids: Vec<PhotoId> = ids.into_iter().map(PhotoId).collect();
        self.catalog().remove_keyword(&ids, keyword)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn library(dir: &Path) -> (std::sync::Arc<Library>, Vec<i64>) {
        let library = Library::open(
            dir.join("data").display().to_string(),
            dir.join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        let charts =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/rawmakase/tests/corpus/charts");
        for name in ["canon-eos-r100-d65.dng", "sony-ilce-7rm5-d65.dng"] {
            std::fs::copy(charts.join(name), day.join(name)).unwrap();
        }
        library.add_new_photos().unwrap();
        let ids = library.photos().unwrap().iter().map(|p| p.id).collect();
        (library, ids)
    }

    #[test]
    fn text_and_keywords_are_kept_per_photo_and_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let (library, ids) = library(dir.path());
        library
            .set_metadata_text(vec![ids[0]], MetadataField::Title, " Harbour ".into())
            .unwrap();
        library
            .set_metadata_text(ids.clone(), MetadataField::Creator, "Ann; Bo".into())
            .unwrap();
        library
            .add_keywords(ids.clone(), "Places > Berlin, boats,  ".into())
            .unwrap();
        drop(library);

        let library = Library::open(
            dir.path().join("data").display().to_string(),
            dir.path().join("caches").display().to_string(),
        )
        .unwrap();
        let first = library.metadata(ids[0]).unwrap();
        assert_eq!(first.title, "Harbour");
        assert_eq!(first.creator, "Ann; Bo");
        let paths: Vec<&str> = first.keywords.iter().map(|k| k.path.as_str()).collect();
        assert_eq!(paths, ["Places > Berlin", "boats"]);
        assert_eq!(
            library.metadata(ids[1]).unwrap().title,
            "",
            "another photo's"
        );

        // Taken off one photo, the keyword stays on the other.
        let berlin = first.keywords[0].id;
        library.remove_keyword(vec![ids[0]], berlin).unwrap();
        assert_eq!(library.metadata(ids[0]).unwrap().keywords.len(), 1);
        assert_eq!(library.metadata(ids[1]).unwrap().keywords.len(), 2);

        // Emptied, a field is cleared.
        library
            .set_metadata_text(vec![ids[0]], MetadataField::Title, "".into())
            .unwrap();
        assert_eq!(library.metadata(ids[0]).unwrap().title, "");
    }

    #[test]
    fn camera_info_is_read_from_the_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let (library, ids) = library(dir.path());
        let camera = library.metadata(ids[0]).unwrap().camera;
        assert!(!camera.is_empty(), "the chart's camera");
        assert!(
            library
                .catalog()
                .photo_info(PhotoId(ids[0]))
                .unwrap()
                .is_some(),
            "kept in the catalog"
        );
    }
}
