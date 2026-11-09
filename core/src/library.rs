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
    /// Lightroom's flag: 1 picked, -1 rejected, 0 unflagged.
    pub flag: i32,
    /// Lightroom's colour label ("Red", "Yellow", "Green", "Blue", "Purple"),
    /// or empty.
    pub label: String,
    /// A virtual copy's name ("Copy 1"); empty for a master.
    pub copy_name: String,
    /// A virtual copy: the same file as its master, with its own edit.
    pub is_copy: bool,
    /// It has an edit made in RAWmakase or Safelight.
    pub edited: bool,
    /// Width over height as shown; 0 until its file's info is read
    /// ([`Library::fill_photo_info`]).
    pub aspect: f32,
    /// Its folder stack (0 when in none), its place in it (0 the top), how
    /// many photos it has and whether it is collapsed to its top.
    pub stack: i64,
    pub stack_index: u32,
    pub stack_count: u32,
    pub stack_collapsed: bool,
}

/// A collection and its photos.
#[derive(uniffi::Record)]
pub struct CollectionInfo {
    pub id: i64,
    pub name: String,
    pub photos: Vec<i64>,
    pub quick: bool,
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
    /// The settings Copy Settings copied last, for Paste.
    pub(crate) copied: crate::transfer::Copied,
    /// The photo left last in the editor, for Paste from Previous.
    pub(crate) previous: crate::transfer::Copied,
    /// AI Denoise and the selection models.
    pub(crate) ai: crate::ai::Ai,
}

#[uniffi::export]
impl Library {
    /// Opens the catalog in `data`, creating it and its photo folder the first
    /// time; thumbnails are cached under `caches`.
    #[uniffi::constructor]
    pub fn open(data: String, caches: String) -> Result<std::sync::Arc<Self>, RenderError> {
        let data = PathBuf::from(data);
        let file = data.join("Catalog").join("Photos.rawmakase");
        // Beside the data folder, not in it: the photos are the user's
        // files, and RAWmakase's file operations leave its own folders alone.
        let photos = photo_folder(&data);
        let caches = PathBuf::from(caches);
        let thumbnails = caches.join("Thumbnails");
        let catalog = on_core_thread(move || {
            move_out_of_data(&data, &photos)?;
            std::fs::create_dir_all(&photos).map_err(anyhow::Error::from)?;
            let mut catalog = if file.exists() {
                Catalog::open(file.as_path())?
            } else {
                Catalog::create(&file)?
            };
            follow_container(&mut catalog, &photos, &data)?;
            // A catalog made before raster masks existed can't keep a selection.
            catalog.upgrade_for_raster_masks()?;
            Ok((catalog, photos))
        });
        let (catalog, photos) = catalog?;
        std::fs::create_dir_all(&thumbnails)
            .map_err(|e| RenderError::from(anyhow::Error::from(e)))?;
        // Selections' rasters are kept in the catalog; the mask store reads
        // them back from it when an edit names one.
        rawmakase_model::storage::mask_assets::add_loader(catalog.mask_asset_loader());
        // AI Denoise's cache is a folder of the decode cache's, which it
        // doesn't create.
        let decoded = caches.join("Decoded");
        std::fs::create_dir_all(&decoded).map_err(|e| RenderError::from(anyhow::Error::from(e)))?;
        let ai = crate::ai::Ai::new(&decoded);
        let library = std::sync::Arc::new(Self {
            catalog: Mutex::new(catalog),
            photos,
            thumbnails,
            copied: Default::default(),
            previous: Default::default(),
            ai,
        });
        // What a crash cut short of a file operation, as the desktop decides
        // it when it opens a catalog; what can't be decided waits.
        library.recover_file_operations()?;
        Ok(library)
    }

    /// The folder the app copies imported originals into, a folder per day.
    pub fn photos_folder(&self) -> String {
        self.photos.display().to_string()
    }

    /// Adds the photos in the photo folder that the catalog doesn't have yet.
    pub fn add_new_photos(&self) -> Result<Added, RenderError> {
        let t = Instant::now();
        let mut catalog = self.catalog();
        let (added, _) = catalog.add_folder_with(&self.photos, &NewPhotoDefaults::default())?;
        drop(catalog);
        self.fill_capture_times()?;
        Ok(Added {
            added: added as u32,
            ms: t.elapsed().as_secs_f64() * 1000.,
        })
    }

    /// Every photo, oldest capture first.
    pub fn photos(&self) -> Result<Vec<LibraryPhoto>, RenderError> {
        let catalog = self.catalog();
        let edited = catalog.edit_times()?;
        let aspects = catalog.aspect_ratios()?;
        let stacks = catalog.stack_places()?;
        let listed = catalog.photos()?;
        drop(catalog);
        let mut photos: Vec<LibraryPhoto> = listed
            .into_iter()
            .map(|p| LibraryPhoto {
                path: p.path.display().to_string(),
                filename: p.filename,
                captured: p.captured,
                rating: p.rating,
                flag: p.flag,
                label: p.label,
                copy_name: p.copy_name,
                is_copy: p.master.is_some(),
                edited: edited.contains_key(&p.id),
                aspect: aspects.get(&p.id).copied().unwrap_or(0.),
                stack: stacks.get(&p.id).map_or(0, |s| s.id.0),
                stack_index: stacks.get(&p.id).map_or(0, |s| s.index as u32),
                stack_count: stacks.get(&p.id).map_or(0, |s| s.count as u32),
                stack_collapsed: stacks.get(&p.id).is_some_and(|s| s.collapsed),
                id: p.id.0,
            })
            .collect();
        photos.sort_by(|a, b| (&a.captured, &a.filename).cmp(&(&b.captured, &b.filename)));
        Ok(photos)
    }

    /// Reads the camera info and size of photos that have none yet, as the
    /// desktop does in the background after adding photos, up to `limit` of
    /// them; returns how many it read. A file that can't be read now is
    /// tried again next time.
    pub fn fill_photo_info(&self, limit: u32) -> Result<u32, RenderError> {
        let todo: Vec<(PhotoId, PathBuf)> = {
            let catalog = self.catalog();
            let missing: std::collections::HashSet<PhotoId> =
                catalog.photos_to_read_info()?.into_iter().collect();
            catalog
                .photos()?
                .into_iter()
                .filter(|p| missing.contains(&p.id))
                .take(limit as usize)
                .map(|p| (p.id, p.path))
                .collect()
        };
        let read = on_core_thread(move || {
            Ok(todo
                .into_iter()
                .filter_map(|(id, path)| {
                    Some((id, rawmakase_decode::photo_info::read_info(&path, local)?))
                })
                .collect::<Vec<_>>())
        })?;
        self.catalog().fill_photo_info(&read)?;
        Ok(read.len() as u32)
    }

    /// Reads the capture time of every photo that has none yet, as the
    /// desktop does in the background after an import: the EXIF date, else
    /// the RAW container's own (CR3 and others). Returns how many it found.
    pub fn fill_capture_times(&self) -> Result<u32, RenderError> {
        let (todo, unread) = {
            let catalog = self.catalog();
            let unread: std::collections::HashSet<PhotoId> =
                catalog.photos_to_read_exif_capture()?.into_iter().collect();
            (catalog.photos()?, unread)
        };
        let reads: Vec<(PhotoId, Option<String>)> = todo
            .into_iter()
            .filter(|p| p.master.is_none() && (p.captured.is_empty() || unread.contains(&p.id)))
            .map(|p| {
                let time = rawmakase_interop::exif::capture_time(&p.path)
                    .or_else(|| rawmakase_decode::raw::capture_time(&p.path));
                (p.id, time)
            })
            .collect();
        let found = reads.iter().filter(|(_, t)| t.is_some()).count() as u32;
        if !reads.is_empty() {
            self.catalog().record_capture_reads(&reads)?;
        }
        Ok(found)
    }

    /// Sets a photo's flag: 1 picked, -1 rejected, 0 none.
    pub fn set_flag(&self, id: i64, flag: i32) -> Result<(), RenderError> {
        self.change_metadata(id, |_, f, _| *f = flag.clamp(-1, 1))
    }

    /// Sets a photo's colour label, or clears it with an empty one.
    pub fn set_label(&self, id: i64, label: String) -> Result<(), RenderError> {
        self.change_metadata(id, |_, _, l| *l = label)
    }

    /// Every collection, with its photos; the Quick Collection first.
    pub fn collections(&self) -> Result<Vec<CollectionInfo>, RenderError> {
        let mut catalog = self.catalog();
        let quick = catalog.quick_collection()?;
        let members = catalog.collection_photos()?;
        let mut list: Vec<CollectionInfo> = catalog
            .collections()?
            .into_iter()
            .filter(|c| {
                c.id == quick || c.kind == rawmakase_catalog::catalog::CollectionKind::Collection
            })
            .map(|c| CollectionInfo {
                quick: c.id == quick,
                name: if c.id == quick {
                    "Quick Collection".into()
                } else {
                    c.name
                },
                photos: members
                    .get(&c.id)
                    .map(|m| m.iter().map(|p| p.0).collect())
                    .unwrap_or_default(),
                id: c.id.0,
            })
            .collect();
        list.sort_by_key(|c| (!c.quick, c.name.to_lowercase()));
        Ok(list)
    }

    /// Adds photos `ids` to the Quick Collection, or takes them out.
    pub fn set_quick_collection(&self, ids: Vec<i64>, on: bool) -> Result<(), RenderError> {
        let mut catalog = self.catalog();
        let quick = catalog.quick_collection()?;
        let ids: Vec<PhotoId> = ids.into_iter().map(PhotoId).collect();
        if on {
            catalog.change_collection(quick, &ids, &[])?;
        } else {
            catalog.change_collection(quick, &[], &ids)?;
        }
        Ok(())
    }

    /// Sets a photo's star rating, 0 to 5, keeping its flag and label.
    pub fn set_rating(&self, id: i64, rating: i32) -> Result<(), RenderError> {
        self.change_metadata(id, |r, _, _| *r = rating)
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
    /// Where full-size decodes are cached for exports: in Caches, which
    /// iPadOS may empty, and kept small.
    pub(crate) fn decode_cache(&self) -> rawmakase_decode::decode_cache::DecodeCache {
        let dir = self.thumbnails.with_file_name("Decoded");
        rawmakase_decode::decode_cache::DecodeCache::new(dir, 2 << 30)
    }

    /// Changes a photo's rating, flag or label, keeping the others, in one
    /// transaction.
    fn change_metadata(
        &self,
        id: i64,
        change: impl FnOnce(&mut i32, &mut i32, &mut String),
    ) -> Result<(), RenderError> {
        let mut catalog = self.catalog();
        let photo = catalog
            .photos()?
            .into_iter()
            .find(|p| p.id.0 == id)
            .ok_or_else(|| RenderError::Failed {
                message: format!("No photo {id} in the catalog"),
            })?;
        let (mut rating, mut flag, mut label) = (photo.rating, photo.flag, photo.label);
        change(&mut rating, &mut flag, &mut label);
        catalog.set_metadata_of(&[(PhotoId(id), rating.clamp(0, 5), flag, label)])?;
        Ok(())
    }

    /// The folder the catalog and the photo folder are in.
    pub(crate) fn data_dir(&self) -> PathBuf {
        self.photos
            .parent()
            .map_or_else(|| self.photos.clone(), Path::to_path_buf)
    }

    /// The catalog, for the open photo's edit to load and save through the
    /// same connection.
    pub(crate) fn catalog(&self) -> std::sync::MutexGuard<'_, Catalog> {
        self.catalog.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The caches folder: thumbnails and other renders kept for speed.
    pub(crate) fn caches_dir(&self) -> PathBuf {
        self.thumbnails
            .parent()
            .map_or_else(|| self.thumbnails.clone(), Path::to_path_buf)
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

/// Points the catalog's photo folder at `photos` when the app's container has
/// moved. iPadOS gives an app a new container path when it is updated or
/// reinstalled, with the files moved along; the catalog still names the old
/// one. A root whose folder is gone and was an earlier container's photo
/// folder is relinked here, as the desktop's Find Missing Folder does.
fn follow_container(catalog: &mut Catalog, photos: &Path, data: &Path) -> anyhow::Result<()> {
    for (id, original, location) in catalog.roots()? {
        let at = PathBuf::from(location.unwrap_or(original));
        // In the data folder, as before the photo folder moved out of it, or
        // beside it, either in an earlier container.
        // The data folder as the catalog stored it, resolved (/private/var).
        let inside = |d: &Path| at == d.join("Photos");
        let ours = inside(data)
            || data.canonicalize().is_ok_and(|d| inside(&d))
            || at.ends_with("Library/Application Support/Safelight/Photos")
            || at.ends_with("Library/Application Support/Photos");
        if at != photos && !at.exists() && ours {
            catalog.relink_root(id, photos)?;
        }
    }
    Ok(())
}

/// The library's photo folder for data folder `data`: "Photos" beside it.
fn photo_folder(data: &Path) -> PathBuf {
    data.parent().unwrap_or(data).join("Photos")
}

/// Moves a photo folder kept in the data folder, as before, out beside it:
/// one rename on the same volume, which the catalog then follows
/// ([`follow_container`]). Left where it is when `photos` is taken.
fn move_out_of_data(data: &Path, photos: &Path) -> anyhow::Result<()> {
    let old = data.join("Photos");
    if old.is_dir() && !photos.exists() {
        std::fs::rename(&old, photos)?;
    }
    Ok(())
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
/// For [`rawmakase_decode::photo_info::read_info`]: the app's photos are in its
/// own storage, never on a network volume, so a RAW is read whole, never from
/// its header alone.
pub(crate) fn local(_: &Path) -> bool {
    false
}

pub(crate) fn fnv(bytes: &[u8]) -> u64 {
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
    fn the_photo_folder_moves_out_of_the_data_folder_with_its_photos() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        // The layout before: the photo folder inside the data folder.
        let old = data.join("Photos").join("d");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::copy(&charts()[0], old.join("a.dng")).unwrap();
        {
            let mut catalog = Catalog::create(&data.join("Catalog/Photos.rawmakase")).unwrap();
            catalog
                .add_folder_with(&data.join("Photos"), &NewPhotoDefaults::default())
                .unwrap();
            let id = catalog.photos().unwrap()[0].id;
            catalog
                .set_metadata_of(&[(id, 3, 0, String::new())])
                .unwrap();
        }
        let library = open(dir.path());
        let folder = PathBuf::from(library.photos_folder());
        assert!(!folder.starts_with(&data), "{}", folder.display());
        assert!(!data.join("Photos").exists());
        let photos = library.photos().unwrap();
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].rating, 3);
        assert!(Path::new(&photos[0].path).exists(), "{}", photos[0].path);
    }

    #[test]
    fn photos_say_whether_they_are_edited_and_their_shape_once_read() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::copy(&charts()[0], day.join("a.dng")).unwrap();
        library.add_new_photos().unwrap();
        let photo = &library.photos().unwrap()[0];
        assert!(!photo.edited);
        assert_eq!(photo.aspect, 0.);
        assert_eq!(library.fill_photo_info(10).unwrap(), 1);
        assert_eq!(library.fill_photo_info(10).unwrap(), 0, "read once");
        let photo = &library.photos().unwrap()[0];
        assert!(photo.aspect > 0.5 && photo.aspect < 2., "{}", photo.aspect);
    }

    #[test]
    fn photos_stack_under_their_top_and_come_apart() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        for (i, chart) in charts().iter().enumerate() {
            std::fs::copy(chart, day.join(format!("{i}.dng"))).unwrap();
        }
        std::fs::copy(&charts()[0], day.join("2.dng")).unwrap();
        library.add_new_photos().unwrap();
        let ids: Vec<i64> = library.photos().unwrap().iter().map(|p| p.id).collect();
        library.stack_photos(vec![ids[2], ids[0], ids[1]]).unwrap();
        let photos = library.photos().unwrap();
        let place = |id: i64| photos.iter().find(|p| p.id == id).unwrap();
        assert_eq!(
            (place(ids[2]).stack_index, place(ids[2]).stack_count),
            (0, 3),
            "the first is the top"
        );
        assert!(place(ids[0]).stack_collapsed && place(ids[0]).stack == place(ids[2]).stack);
        library
            .set_stack_collapsed(place(ids[0]).stack, false)
            .unwrap();
        library.move_to_top_of_stack(ids[1]).unwrap();
        let photos = library.photos().unwrap();
        let place = |id: i64| photos.iter().find(|p| p.id == id).unwrap();
        assert_eq!(place(ids[1]).stack_index, 0);
        assert!(!place(ids[1]).stack_collapsed);
        library.unstack(vec![ids[0]]).unwrap();
        assert!(library.photos().unwrap().iter().all(|p| p.stack == 0));
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
    fn the_grid_sorts_as_the_desktop_does() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let day = Path::new(&library.photos_folder()).join("2026-11-08");
        std::fs::create_dir_all(&day).unwrap();
        let [first, second] = charts();
        for (chart, name) in [
            (&first, "IMG_10.dng"),
            (&second, "IMG_9.dng"),
            (&first, "a.dng"),
        ] {
            std::fs::copy(chart, day.join(name)).unwrap();
        }
        library.add_new_photos().unwrap();
        let photos = library.photos().unwrap();
        let id = |name: &str| photos.iter().find(|p| p.filename == name).unwrap().id;
        library.set_rating(id("IMG_10.dng"), 5).unwrap();
        let orders = sort_orders();
        let index = |name: &str| orders.iter().find(|o| o.name == name).unwrap().index;
        assert_eq!(orders[0].name, "Capture Time");
        assert_eq!(
            library.photo_order(index("File Name"), false).unwrap(),
            [id("a.dng"), id("IMG_9.dng"), id("IMG_10.dng")],
            "digits by value"
        );
        assert_eq!(
            library.photo_order(index("Rating"), true).unwrap()[0],
            id("IMG_10.dng")
        );
        assert_eq!(
            library
                .photo_order(index("Capture Time"), false)
                .unwrap()
                .len(),
            3
        );
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
    fn photos_are_found_after_the_container_moves() {
        let dir = tempfile::tempdir().unwrap();
        // Containers keep the app's own folder layout below them.
        let old = dir.path().join("A/Library/Application Support/Safelight");
        let new = dir.path().join("B/Library/Application Support/Safelight");
        let library = Library::open(
            old.display().to_string(),
            dir.path().join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::copy(&charts()[0], day.join("a.dng")).unwrap();
        library.add_new_photos().unwrap();
        drop(library);

        // The whole container moves.
        std::fs::rename(dir.path().join("A"), dir.path().join("B")).unwrap();
        let moved = Library::open(
            new.display().to_string(),
            dir.path().join("caches").display().to_string(),
        )
        .unwrap();
        let photos = moved.photos().unwrap();
        assert_eq!(photos.len(), 1);
        assert!(Path::new(&photos[0].path).exists(), "{}", photos[0].path);
        assert!(
            photos[0]
                .path
                .starts_with(new.parent().unwrap().to_str().unwrap())
        );
    }

    #[test]
    fn flags_labels_and_the_quick_collection_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        for chart in charts() {
            std::fs::copy(&chart, day.join(chart.file_name().unwrap())).unwrap();
        }
        library.add_new_photos().unwrap();
        let ids: Vec<i64> = library.photos().unwrap().iter().map(|p| p.id).collect();
        library.set_rating(ids[0], 3).unwrap();
        library.set_flag(ids[0], 1).unwrap();
        library.set_label(ids[0], "Red".into()).unwrap();
        library.set_flag(ids[1], -1).unwrap();
        library.set_quick_collection(vec![ids[1]], true).unwrap();
        drop(library);

        let reopened = open(dir.path());
        let photos = reopened.photos().unwrap();
        let first = photos.iter().find(|p| p.id == ids[0]).unwrap();
        assert_eq!(
            (first.rating, first.flag, first.label.as_str()),
            (3, 1, "Red")
        );
        assert_eq!(photos.iter().find(|p| p.id == ids[1]).unwrap().flag, -1);
        let quick = reopened
            .collections()
            .unwrap()
            .into_iter()
            .find(|c| c.quick)
            .unwrap();
        assert_eq!(quick.photos, [ids[1]]);
    }

    #[test]
    fn fit_never_enlarges() {
        assert_eq!(fit(100, 50, 512), (100, 50));
        assert_eq!(fit(4000, 3000, 400), (400, 300));
        assert_eq!(fit(3000, 4000, 400), (300, 400));
    }
}

/// A sort order the grid offers, as Lightroom names it.
#[derive(uniffi::Record)]
pub struct SortChoice {
    /// What [`Library::photo_order`] takes.
    pub index: u32,
    pub name: String,
}

/// Lightroom's sort orders, in its menu's order (RAWmakase's
/// [`Sort`](rawmakase_catalog::catalog::sort::Sort)).
#[uniffi::export]
pub fn sort_orders() -> Vec<SortChoice> {
    rawmakase_catalog::catalog::sort::Sort::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| SortChoice {
            index: i as u32,
            name: s.name().to_string(),
        })
        .collect()
}

#[uniffi::export]
impl Library {
    /// Every photo's id in sort order `sort` (from [`sort_orders`]), or the
    /// other way round when `reverse`, as the desktop's grid sorts: photos
    /// alike in it stay in capture order.
    pub fn photo_order(&self, sort: u32, reverse: bool) -> Result<Vec<i64>, RenderError> {
        use rawmakase_catalog::catalog::sort::Sort;
        let sort = Sort::ALL.get(sort as usize).copied().unwrap_or_default();
        let catalog = self.catalog();
        let mut photos = catalog.photos()?;
        // Capture order first, as the desktop's catalog lists them.
        photos.sort_by(|a, b| (&a.captured, &a.filename).cmp(&(&b.captured, &b.filename)));
        let keys = sort.keys(&catalog);
        drop(catalog);
        let mut indices: Vec<usize> = (0..photos.len()).collect();
        sort.sort(&photos, &mut indices, &keys, reverse);
        Ok(indices.into_iter().map(|i| photos[i].id.0).collect())
    }
}

#[uniffi::export]
impl Library {
    /// Lightroom's Create Virtual Copy: a photo of the same file with photo
    /// `id`'s edit, rating, flag, label, keywords and metadata, named "Copy
    /// N". Returns its id.
    pub fn create_virtual_copy(&self, id: i64) -> Result<i64, RenderError> {
        Ok(self.catalog().create_virtual_copy(PhotoId(id))?.0)
    }

    /// Renames a virtual copy.
    pub fn rename_virtual_copy(&self, id: i64, name: String) -> Result<(), RenderError> {
        self.catalog().set_copy_name(PhotoId(id), name.trim())?;
        Ok(())
    }

    /// Lightroom's Set Copy as Master: the copy becomes the master, the
    /// master and the other copies its copies.
    pub fn set_copy_as_master(&self, id: i64) -> Result<(), RenderError> {
        self.catalog().set_copy_as_master(PhotoId(id))?;
        Ok(())
    }

    /// Removes a virtual copy from the catalog; its file, which its master
    /// shows, stays.
    pub fn remove_virtual_copy(&self, id: i64) -> Result<(), RenderError> {
        self.catalog().remove_virtual_copy(PhotoId(id))?;
        Ok(())
    }
}

/// Lightroom's folder stacks, as the desktop's catalog keeps them: photos of
/// one folder under the first, the top, shown alone while the stack is
/// collapsed. Organization only: nothing of a stack is in an edit.
#[uniffi::export]
impl Library {
    /// Group into Stack: `ids` become one collapsed stack in this order,
    /// the first its top; photos of other folders than the first's stay
    /// out, as Lightroom's stacks are a folder's.
    pub fn stack_photos(&self, ids: Vec<i64>) -> Result<(), RenderError> {
        use rawmakase_catalog::catalog::StackChange;
        self.catalog()
            .change_stacks(&StackChange::Group(ids.into_iter().map(PhotoId).collect()))?;
        Ok(())
    }

    /// Unstack: every stack one of `ids` is in comes apart.
    pub fn unstack(&self, ids: Vec<i64>) -> Result<(), RenderError> {
        use rawmakase_catalog::catalog::StackChange;
        self.catalog().change_stacks(&StackChange::Unstack(
            ids.into_iter().map(PhotoId).collect(),
        ))?;
        Ok(())
    }

    /// Move to Top of Stack.
    pub fn move_to_top_of_stack(&self, id: i64) -> Result<(), RenderError> {
        use rawmakase_catalog::catalog::StackChange;
        self.catalog()
            .change_stacks(&StackChange::MoveToTop(PhotoId(id)))?;
        Ok(())
    }

    /// Collapses stack `stack` to its top, or expands it.
    pub fn set_stack_collapsed(&self, stack: i64, collapsed: bool) -> Result<(), RenderError> {
        self.catalog()
            .set_stacks_collapsed(&[rawmakase_catalog::catalog::StackId(stack)], collapsed)?;
        Ok(())
    }

    /// Auto-Stack by Capture Time: runs of a folder's photos, of `ids`,
    /// each taken less than `seconds` after the one before, become stacks.
    /// Returns how many.
    pub fn auto_stack(&self, ids: Vec<i64>, seconds: f64) -> Result<u32, RenderError> {
        use rawmakase_catalog::catalog::{StackChange, auto_stacks};
        let mut catalog = self.catalog();
        let photos = catalog.photos()?;
        let chosen: Vec<(PhotoId, rawmakase_catalog::catalog::FolderId, &str)> = photos
            .iter()
            .filter(|p| ids.contains(&p.id.0) && p.master.is_none())
            .map(|p| (p.id, p.folder, p.captured.as_str()))
            .collect();
        let stacks = auto_stacks(&chosen, seconds);
        let made = stacks.len() as u32;
        if made > 0 {
            catalog.change_stacks(&StackChange::Groups(stacks))?;
        }
        Ok(made)
    }
}
