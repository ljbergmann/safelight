//! Stage 4: a RAWmakase catalog in the app's own storage.
//!
//! The catalog is an ordinary `.rawmakase` file, the desktop's format, in
//! the app's data folder. Imported originals are copied by the app into
//! `Photos/<day>/` beside it, and the whole `Photos` folder is one root of
//! the catalog, so adding it again adds only the photos that are new.
//! Thumbnails are made from each RAW's smallest embedded preview that is
//! large enough, and cached in the app's Caches folder, which iPadOS may
//! empty.

use crate::render::{RenderError, on_core_thread};
use rawmakase_catalog::catalog::{Catalog, NewPhotoDefaults, PhotoId};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

/// A photo of the catalog, as the grid shows it.
#[derive(uniffi::Record)]
pub struct LibraryPhoto {
    pub id: i64,
    pub path: String,
    pub filename: String,
    /// Capture time as the catalog stores it; empty when unknown.
    pub captured: String,
    /// 0 to 5 stars.
    pub rating: i32,
}

/// What adding the new photos did.
#[derive(uniffi::Record)]
pub struct Added {
    pub added: u32,
    pub ms: f64,
}

/// The catalog and the folders it keeps its photos in.
#[derive(uniffi::Object)]
pub struct Library {
    catalog: Mutex<Catalog>,
    photos: PathBuf,
    thumbnails: PathBuf,
}

#[uniffi::export]
impl Library {
    /// Opens the catalog in `data`, creating it and its photo folder the first
    /// time; thumbnails are cached under `caches`.
    #[uniffi::constructor]
    pub fn open(data: String, caches: String) -> Result<std::sync::Arc<Self>, RenderError> {
        let data = PathBuf::from(data);
        let file = data.join("Catalog").join("Photos.rawmakase");
        let photos = data.join("Photos");
        let thumbnails = PathBuf::from(caches).join("Thumbnails");
        let catalog = on_core_thread(move || {
            std::fs::create_dir_all(&photos).map_err(anyhow::Error::from)?;
            let catalog = if file.exists() {
                Catalog::open(file.as_path())?
            } else {
                Catalog::create(&file)?
            };
            Ok((catalog, photos))
        });
        let (catalog, photos) = catalog?;
        std::fs::create_dir_all(&thumbnails)
            .map_err(|e| RenderError::from(anyhow::Error::from(e)))?;
        Ok(std::sync::Arc::new(Self {
            catalog: Mutex::new(catalog),
            photos,
            thumbnails,
        }))
    }

    /// The folder the app copies imported originals into, a folder per day.
    pub fn photos_folder(&self) -> String {
        self.photos.display().to_string()
    }

    /// Adds the photos in the photo folder that the catalog doesn't have yet.
    pub fn add_new_photos(&self) -> Result<Added, RenderError> {
        let t = Instant::now();
        let mut catalog = self.lock();
        let (added, _) = catalog.add_folder_with(&self.photos, &NewPhotoDefaults::default())?;
        Ok(Added {
            added: added as u32,
            ms: t.elapsed().as_secs_f64() * 1000.,
        })
    }

    /// Every photo, oldest capture first.
    pub fn photos(&self) -> Result<Vec<LibraryPhoto>, RenderError> {
        let mut photos: Vec<LibraryPhoto> = self
            .lock()
            .photos()?
            .into_iter()
            .map(|p| LibraryPhoto {
                id: p.id.0,
                path: p.path.display().to_string(),
                filename: p.filename,
                captured: p.captured,
                rating: p.rating,
            })
            .collect();
        photos.sort_by(|a, b| (&a.captured, &a.filename).cmp(&(&b.captured, &b.filename)));
        Ok(photos)
    }

    /// Sets a photo's star rating, 0 to 5, keeping its flag and label.
    pub fn set_rating(&self, id: i64, rating: i32) -> Result<(), RenderError> {
        let mut catalog = self.lock();
        let photo = catalog
            .photos()?
            .into_iter()
            .find(|p| p.id.0 == id)
            .ok_or_else(|| RenderError::Failed {
                message: format!("No photo {id} in the catalog"),
            })?;
        catalog.set_metadata_of(&[(PhotoId(id), rating.clamp(0, 5), photo.flag, photo.label)])?;
        Ok(())
    }

    /// A JPEG of the photo's embedded preview, `edge` pixels on its long side,
    /// upright; made once per file version and size, then read from the cache.
    pub fn thumbnail(&self, path: String, edge: u32) -> Result<Vec<u8>, RenderError> {
        let cached = self.thumbnail_path(Path::new(&path), edge)?;
        if let Ok(bytes) = std::fs::read(&cached) {
            return Ok(bytes);
        }
        on_core_thread(move || {
            // As the desktop's Library: the smallest embedded preview that
            // reaches `edge`, so a camera's full-size JPEG is decoded only
            // when nothing smaller is there; else the largest.
            let mut raw = rawmakase_decode::raw::Raw::open_file(Path::new(&path))?;
            let preview = match rawmakase_decode::raw::thumbnail_reaching(&mut raw, edge) {
                Ok(preview) => preview,
                Err(_) => rawmakase_decode::raw::thumbnail(&mut raw)?,
            };
            let small = image::imageops::thumbnail(
                &preview,
                fit(preview.width(), preview.height(), edge).0,
                fit(preview.width(), preview.height(), edge).1,
            );
            let mut jpeg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
                .encode_image(&small)
                .map_err(anyhow::Error::from)?;
            // A failed write only means the next call makes it again.
            let _ = std::fs::write(&cached, &jpeg);
            Ok(jpeg)
        })
    }
}

impl Library {
    fn lock(&self) -> std::sync::MutexGuard<'_, Catalog> {
        self.catalog.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cache file for `path` at `edge`, named by the file's size and
    /// modification time, so a replaced file gets a new thumbnail.
    fn thumbnail_path(&self, path: &Path, edge: u32) -> Result<PathBuf, RenderError> {
        let meta = std::fs::metadata(path).map_err(anyhow::Error::from)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "{:016x}-{}-{modified}-{edge}.jpg",
            fnv(path.as_os_str().as_encoded_bytes()),
            meta.len()
        );
        Ok(self.thumbnails.join(name))
    }
}

/// `width` × `height` scaled to fit `edge` on its long side, never enlarged.
fn fit(width: u32, height: u32, edge: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= edge {
        return (width, height);
    }
    let scale = edge as f64 / long as f64;
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
}

/// FNV-1a, to name a cache file after a path.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two of RAWmakase's synthetic chart DNGs, which its repository ships.
    fn charts() -> [PathBuf; 2] {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/rawmakase/tests/corpus/charts");
        [
            dir.join("canon-eos-r100-d65.dng"),
            dir.join("sony-ilce-7rm5-d65.dng"),
        ]
    }

    fn open(dir: &Path) -> std::sync::Arc<Library> {
        Library::open(
            dir.join("data").display().to_string(),
            dir.join("caches").display().to_string(),
        )
        .unwrap()
    }

    #[test]
    fn imported_photos_and_ratings_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let day = Path::new(&library.photos_folder()).join("2026-11-08");
        std::fs::create_dir_all(&day).unwrap();
        for chart in charts() {
            std::fs::copy(&chart, day.join(chart.file_name().unwrap())).unwrap();
        }
        assert_eq!(library.add_new_photos().unwrap().added, 2);
        // Adding again finds nothing new.
        assert_eq!(library.add_new_photos().unwrap().added, 0);
        let photos = library.photos().unwrap();
        assert_eq!(photos.len(), 2);
        library.set_rating(photos[1].id, 4).unwrap();
        drop(library);

        let reopened = open(dir.path());
        let photos = reopened.photos().unwrap();
        assert_eq!(photos.len(), 2);
        assert_eq!(photos.iter().map(|p| p.rating).collect::<Vec<_>>(), [0, 4]);
    }

    #[test]
    fn a_later_import_adds_only_its_own_photos() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let [first, second] = charts();
        for (day, chart) in [("2026-11-08", &first), ("2026-11-09", &second)] {
            let folder = Path::new(&library.photos_folder()).join(day);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::copy(chart, folder.join(chart.file_name().unwrap())).unwrap();
            assert_eq!(library.add_new_photos().unwrap().added, 1);
        }
        assert_eq!(library.photos().unwrap().len(), 2);
    }

    #[test]
    fn a_rating_outside_zero_to_five_is_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::copy(&charts()[0], day.join("a.dng")).unwrap();
        library.add_new_photos().unwrap();
        let id = library.photos().unwrap()[0].id;
        library.set_rating(id, 9).unwrap();
        assert_eq!(library.photos().unwrap()[0].rating, 5);
    }

    #[test]
    fn fit_never_enlarges() {
        assert_eq!(fit(100, 50, 512), (100, 50));
        assert_eq!(fit(4000, 3000, 400), (400, 300));
        assert_eq!(fit(3000, 4000, 400), (300, 400));
    }
}
