//! Importing photos from a card, a drive, a folder or the Photos library into
//! the catalog, as Lightroom's Import does it: the source is listed first,
//! with each photo's capture time and whether the catalog has it already;
//! the chosen photos are then copied into `Photos/<year>/<year-month-day>/`
//! by capture date and added, with progress, and a Cancel that stops between
//! files and keeps what was imported.
//!
//! A photo counts as in the catalog when a photo there has the same size and
//! the same quick hash, RAWmakase's own checksum (`storage::FileChecksum`)
//! that the desktop keeps for finding moved files. Every imported photo gets
//! its full checksum, read while it is copied anyway, so the next import and
//! the sync can tell it by content.

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_catalog::catalog::{PhotoId, ReadChecksum};
use rawmakase_model::storage::{ChecksumRead as Outcome, FileChecksum, is_photo};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// A photo in the source, as the import screen lists it.
#[derive(uniffi::Record, Clone, Debug)]
pub struct ImportCandidate {
    pub path: String,
    pub filename: String,
    pub size: u64,
    /// When it was taken, as Lightroom stores it ("2023-04-22T14:03:11.120");
    /// empty when the file doesn't say.
    pub captured: String,
    /// The catalog has a photo with the same content already.
    pub duplicate: bool,
}

#[uniffi::export]
impl Library {
    /// Every photo under `folder` (a card's `DCIM`, say, however deep),
    /// oldest first, each with its capture time and whether the catalog has
    /// it. Reads a few blocks of each file, not the whole.
    pub fn scan_for_import(&self, folder: String) -> Result<Vec<ImportCandidate>, RenderError> {
        let mut files = Vec::new();
        list(Path::new(&folder), &mut files, 0)?;
        let known = self.known_contents()?;
        let mut found: Vec<ImportCandidate> = files
            .into_iter()
            .filter_map(|path| {
                let checksum = match FileChecksum::read_quick(&path, |_| true) {
                    Ok(Outcome::Done(c)) => c,
                    _ => return None,
                };
                Some(ImportCandidate {
                    filename: path.file_name()?.to_string_lossy().into_owned(),
                    captured: capture_time(&path).unwrap_or_default(),
                    duplicate: known.contains(&(checksum.size, checksum.quick.clone())),
                    size: checksum.size,
                    path: path.display().to_string(),
                })
            })
            .collect();
        found.sort_by(|a, b| (&a.captured, &a.filename).cmp(&(&b.captured, &b.filename)));
        Ok(found)
    }
}

impl Library {
    /// The size and quick hash of every photo in the catalog, reading the
    /// checksum of any that has none on record yet (photos imported before
    /// checksums were kept) and saving it.
    fn known_contents(&self) -> Result<HashSet<(u64, String)>, RenderError> {
        let (photos, stored) = {
            let catalog = self.catalog();
            (catalog.photos()?, catalog.checksums()?)
        };
        let mut known: HashSet<(u64, String)> =
            stored.values().map(|c| (c.size, c.quick.clone())).collect();
        let missing: Vec<ReadChecksum> = photos
            .into_iter()
            .filter(|p| p.master.is_none() && !stored.contains_key(&p.id))
            .filter_map(|p| match FileChecksum::read_quick(&p.path, |_| true) {
                Ok(Outcome::Done(checksum)) => Some(ReadChecksum {
                    photo: p.id,
                    folder: p.folder,
                    filename: p.filename,
                    path: p.path,
                    checksum,
                }),
                _ => None,
            })
            .collect();
        known.extend(
            missing
                .iter()
                .map(|r| (r.checksum.size, r.checksum.quick.clone())),
        );
        if !missing.is_empty() {
            self.catalog().save_checksums(&missing)?;
        }
        Ok(known)
    }
}

/// Lists the photos under `dir`, skipping hidden folders (a card's
/// `.Trashes`, `.Spotlight-V100`) and stopping at a sane depth.
fn list(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) -> Result<(), RenderError> {
    if depth > 8 {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(anyhow::Error::from)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_dir() => list(&path, out, depth + 1)?,
            Ok(t) if t.is_file() && is_photo(&path) => out.push(path),
            _ => {}
        }
    }
    Ok(())
}

/// The capture time in Lightroom's form: the EXIF date, else the RAW
/// container's own, as the catalog reads it.
fn capture_time(path: &Path) -> Option<String> {
    rawmakase_interop::exif::capture_time(path)
        .or_else(|| rawmakase_decode::raw::capture_time(path))
}

/// The folder a photo taken at `captured` goes in, `<year>/<year-month-day>`
/// as Lightroom's "By date" does; the file's modification day without one.
fn dated_folder(captured: &str, file: &Path) -> PathBuf {
    let day = captured
        .get(..10)
        // Digits and dashes only: the date comes from the file, and becomes
        // a folder name.
        .filter(|d| {
            d.len() == 10
                && d.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
        })
        .map(str::to_string)
        .or_else(|| {
            let modified = std::fs::metadata(file).and_then(|m| m.modified()).ok()?;
            let days = modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs()
                / 86_400;
            Some(civil_day(days as i64))
        })
        .unwrap_or_else(|| "Undated".into());
    match day.get(..4) {
        Some(year) if day != "Undated" => PathBuf::from(year).join(&day),
        _ => PathBuf::from(day),
    }
}

/// "YYYY-MM-DD" of the day `days` after 1970-01-01 (UTC).
fn civil_day(days: i64) -> String {
    // Howard Hinnant's days-from-civil, inverted.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `wanted` in `folder`, or with " (2)", " (3)"… before the extension when a
/// file of that name is there already.
fn unused(folder: &Path, wanted: &str) -> PathBuf {
    let first = folder.join(wanted);
    if !first.exists() {
        return first;
    }
    let path = Path::new(wanted);
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("some number is free")
}

/// An import running on its own thread; the app polls it.
#[derive(uniffi::Object)]
pub struct ImportJob {
    total: u32,
    total_bytes: u64,
    done: Arc<AtomicU32>,
    bytes: Arc<AtomicU64>,
    imported: Arc<AtomicU32>,
    skipped: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    /// `None` while running, then how it ended.
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
}

/// What an import does with the files it takes.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMode {
    /// Copies them, leaving the source as it was (a card, a drive).
    Copy,
    /// Moves them (files the app was given to keep, as from Photos or the
    /// app's Documents folder).
    Move,
}

#[uniffi::export]
impl ImportJob {
    /// Imports `paths` into `library`: copies (or moves) each into its dated
    /// folder, reading its checksum on the way, skips those whose content the
    /// catalog has when `skip_duplicates`, and adds them to the catalog.
    #[uniffi::constructor]
    pub fn start(
        library: Arc<Library>,
        paths: Vec<String>,
        mode: ImportMode,
        skip_duplicates: bool,
    ) -> Arc<Self> {
        let total_bytes = paths
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        let job = Arc::new(Self {
            total: paths.len() as u32,
            total_bytes,
            done: Arc::default(),
            bytes: Arc::default(),
            imported: Arc::default(),
            skipped: Arc::default(),
            cancel: Arc::default(),
            outcome: Arc::default(),
        });
        let worker = job.clone();
        let spawned = std::thread::Builder::new()
            .name("import".into())
            .stack_size(crate::render::STACK_BYTES)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker.run(&library, &paths, mode, skip_duplicates)
                }));
                let result = match result {
                    Ok(r) => r.map_err(|e| format!("{e:#}")),
                    Err(_) => Err("The import stopped unexpectedly".into()),
                };
                *worker
                    .outcome
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(result);
            });
        if let Err(e) = spawned {
            *job.outcome.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(Err(format!("Couldn't start the import: {e}")));
        }
        job
    }

    /// Files dealt with so far (imported or skipped), of [`Self::total`].
    pub fn done(&self) -> u32 {
        self.done.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Photos added to the catalog.
    pub fn imported(&self) -> u32 {
        self.imported.load(Ordering::Relaxed)
    }

    /// Photos left out as duplicates.
    pub fn skipped(&self) -> u32 {
        self.skipped.load(Ordering::Relaxed)
    }

    pub fn finished(&self) -> bool {
        self.outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Why it stopped, once it has failed; `None` while running, after
    /// success or after Cancel.
    pub fn error(&self) -> Option<String> {
        self.outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|r| r.as_ref().err().cloned())
    }

    /// Stops after the file being copied; what was imported stays.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl ImportJob {
    fn run(
        &self,
        library: &Library,
        paths: &[String],
        mode: ImportMode,
        skip_duplicates: bool,
    ) -> anyhow::Result<()> {
        let mut known = if skip_duplicates {
            library
                .known_contents()
                .map_err(|e| anyhow::anyhow!("{e}"))?
        } else {
            HashSet::new()
        };
        let root = PathBuf::from(library.photos_folder());
        let mut placed: Vec<(PathBuf, FileChecksum)> = Vec::new();
        // Added in batches, so photos show up during a long import and a
        // failure later keeps what was added.
        let add = |placed: &mut Vec<(PathBuf, FileChecksum)>| -> anyhow::Result<()> {
            if placed.is_empty() {
                return Ok(());
            }
            let added = library
                .add_files(placed)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            self.imported.fetch_add(added, Ordering::Relaxed);
            placed.clear();
            Ok(())
        };
        for source in paths {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            let source = Path::new(source);
            let size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
            let quick = match FileChecksum::read_quick(source, |_| true)? {
                Outcome::Done(c) => c,
                _ => anyhow::bail!("{} changed while it was read", source.display()),
            };
            if skip_duplicates && known.contains(&(quick.size, quick.quick.clone())) {
                self.skipped.fetch_add(1, Ordering::Relaxed);
                self.done.fetch_add(1, Ordering::Relaxed);
                self.bytes.fetch_add(size, Ordering::Relaxed);
                continue;
            }
            let captured = capture_time(source).unwrap_or_default();
            let folder = root.join(dated_folder(&captured, source));
            std::fs::create_dir_all(&folder)?;
            let name = source
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("{} has no name", source.display()))?
                .to_string_lossy();
            let target = unused(&folder, &name);
            match mode {
                ImportMode::Move => {
                    // Across volumes a rename fails: copied as Copy does,
                    // so a copy cut short never looks like a photo.
                    if std::fs::rename(source, &target).is_err() {
                        copy(source, &target)?;
                        std::fs::remove_file(source)?;
                    }
                }
                ImportMode::Copy => copy(source, &target)?,
            }
            let checksum = match FileChecksum::read_full(&target, |_| true)? {
                Outcome::Done(c) => c,
                _ => anyhow::bail!("{} changed while it was read", target.display()),
            };
            if checksum.quick != quick.quick {
                let _ = std::fs::remove_file(&target);
                anyhow::bail!("{} didn't copy intact", source.display());
            }
            known.insert((checksum.size, checksum.quick.clone()));
            placed.push((target, checksum));
            self.done.fetch_add(1, Ordering::Relaxed);
            self.bytes.fetch_add(size, Ordering::Relaxed);
            if placed.len() >= 20 {
                add(&mut placed)?;
            }
        }
        add(&mut placed)?;
        library
            .fill_capture_times()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(())
    }
}

/// Copies `from` to `to` through a temporary name beside it, so a copy cut
/// short (Cancel, a card pulled out) never looks like a photo.
fn copy(from: &Path, to: &Path) -> anyhow::Result<()> {
    let partial = to.with_extension(format!(
        "{}.importing",
        to.extension()
            .map_or_else(String::new, |e| e.to_string_lossy().into_owned())
    ));
    std::fs::copy(from, &partial).inspect_err(|_| {
        let _ = std::fs::remove_file(&partial);
    })?;
    std::fs::rename(&partial, to)?;
    Ok(())
}

impl Library {
    /// Adds files placed in the photo folder to the catalog, with the
    /// checksums read from them. Returns how many photos were added.
    fn add_files(&self, placed: &[(PathBuf, FileChecksum)]) -> Result<u32, RenderError> {
        let before: HashSet<PhotoId> = self.catalog().photos()?.iter().map(|p| p.id).collect();
        self.add_new_photos()?;
        let photos = self.catalog().photos()?;
        let read: Vec<ReadChecksum> = photos
            .iter()
            .filter(|p| !before.contains(&p.id) && p.master.is_none())
            .filter_map(|p| {
                // The catalog keeps paths as the file system resolves them
                // (/private/var for /var on Apple systems).
                let here = p.path.canonicalize().ok()?;
                let (_, checksum) = placed
                    .iter()
                    .find(|(path, _)| path.canonicalize().ok().as_ref() == Some(&here))?;
                Some(ReadChecksum {
                    photo: p.id,
                    folder: p.folder,
                    filename: p.filename.clone(),
                    path: p.path.clone(),
                    checksum: checksum.clone(),
                })
            })
            .collect();
        let added = photos.iter().filter(|p| !before.contains(&p.id)).count() as u32;
        self.catalog().save_checksums(&read)?;
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(dir: &Path) -> Arc<Library> {
        Library::open(
            dir.join("data").display().to_string(),
            dir.join("caches").display().to_string(),
        )
        .unwrap()
    }

    /// A "card" with two of RAWmakase's chart DNGs in a camera's folders.
    fn card(dir: &Path) -> PathBuf {
        let card = dir.join("card");
        let dcim = card.join("DCIM/100CANON");
        std::fs::create_dir_all(&dcim).unwrap();
        std::fs::create_dir_all(card.join(".Trashes")).unwrap();
        let charts =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/rawmakase/tests/corpus/charts");
        for name in ["canon-eos-r100-d65.dng", "sony-ilce-7rm5-d65.dng"] {
            std::fs::copy(charts.join(name), dcim.join(name)).unwrap();
        }
        std::fs::copy(
            charts.join("canon-eos-r100-d65.dng"),
            card.join(".Trashes/deleted.dng"),
        )
        .unwrap();
        std::fs::write(dcim.join("notes.txt"), "not a photo").unwrap();
        card
    }

    fn wait(job: &ImportJob) {
        while !job.finished() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_card_lists_its_photos_and_imports_them_by_date_once() {
        let dir = tempfile::tempdir().unwrap();
        let library = library(dir.path());
        let card = card(dir.path());

        let found = library.scan_for_import(card.display().to_string()).unwrap();
        assert_eq!(
            found.len(),
            2,
            "hidden folders and other files are left out"
        );
        assert!(found.iter().all(|c| !c.duplicate));

        let paths: Vec<String> = found.iter().map(|c| c.path.clone()).collect();
        let job = ImportJob::start(library.clone(), paths.clone(), ImportMode::Copy, true);
        wait(&job);
        assert_eq!(job.error(), None);
        assert_eq!((job.imported(), job.skipped(), job.done()), (2, 0, 2));
        let photos = library.photos().unwrap();
        assert_eq!(photos.len(), 2);
        for (photo, candidate) in photos.iter().zip(&found) {
            // In <year>/<year-month-day> of its capture, or of the file's date.
            let day = Path::new(&photo.path).parent().unwrap();
            let year = day.parent().unwrap();
            assert_eq!(
                year.parent().unwrap().canonicalize().unwrap(),
                Path::new(&library.photos_folder()).canonicalize().unwrap()
            );
            assert!(
                day.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(&*year.file_name().unwrap().to_string_lossy())
            );
            if !candidate.captured.is_empty() {
                assert_eq!(
                    day.file_name().unwrap().to_string_lossy(),
                    candidate.captured[..10]
                );
            }
        }
        // The card is left as it was.
        assert!(Path::new(&paths[0]).exists());

        // Listed again, both are in the catalog; imported again, nothing is.
        let again = library.scan_for_import(card.display().to_string()).unwrap();
        assert!(again.iter().all(|c| c.duplicate));
        // Their full checksums were kept, read while copying.
        let checksums = library.catalog().checksums().unwrap();
        assert_eq!(checksums.len(), 2);
        assert!(checksums.values().all(|c| c.full.is_some()));
        let job = ImportJob::start(library.clone(), paths, ImportMode::Copy, true);
        wait(&job);
        assert_eq!((job.imported(), job.skipped()), (0, 2));
        assert_eq!(library.photos().unwrap().len(), 2);
    }

    #[test]
    fn photos_imported_before_checksums_were_kept_count_as_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let library = library(dir.path());
        let card = card(dir.path());
        // Placed and added the old way, with no checksum on record.
        let old = Path::new(&library.photos_folder()).join("2026-11-08");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::copy(
            card.join("DCIM/100CANON/canon-eos-r100-d65.dng"),
            old.join("IMG_1.dng"),
        )
        .unwrap();
        library.add_new_photos().unwrap();

        let found = library.scan_for_import(card.display().to_string()).unwrap();
        let duplicates: Vec<_> = found
            .iter()
            .filter(|c| c.duplicate)
            .map(|c| c.filename.as_str())
            .collect();
        assert_eq!(duplicates, ["canon-eos-r100-d65.dng"]);
    }

    #[test]
    fn cancel_keeps_what_was_imported_and_leaves_no_partial_file() {
        let dir = tempfile::tempdir().unwrap();
        let library = library(dir.path());
        let card = card(dir.path());
        let paths: Vec<String> = library
            .scan_for_import(card.display().to_string())
            .unwrap()
            .into_iter()
            .map(|c| c.path)
            .collect();
        let job = ImportJob::start(library.clone(), paths, ImportMode::Copy, true);
        job.cancel();
        wait(&job);
        assert_eq!(job.error(), None);
        assert_eq!(library.photos().unwrap().len() as u32, job.imported());
        let mut leftovers = Vec::new();
        let mut stack = vec![PathBuf::from(library.photos_folder())];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else if e.path().to_string_lossy().ends_with(".importing") {
                    leftovers.push(e.path());
                }
            }
        }
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// What the Photos import hands over: each original copied out of the
    /// Photos library into a folder of its own, under the name Photos
    /// suggests, which two photos can share.
    #[test]
    fn originals_from_photos_move_in_under_their_names_and_duplicates_stay_out() {
        let dir = tempfile::tempdir().unwrap();
        let library = library(dir.path());
        let charts =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/rawmakase/tests/corpus/charts");
        let staged = dir.path().join("staged");
        let mut paths = Vec::new();
        for (folder, chart, name) in [
            ("0", "canon-eos-r100-d65.dng", "IMG_0001.DNG"),
            ("1", "sony-ilce-7rm5-d65.dng", "IMG_0001.DNG"),
            // The same photo picked twice, or already in the catalog.
            ("2", "canon-eos-r100-d65.dng", "IMG_0002.DNG"),
        ] {
            let folder = staged.join(folder);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::copy(charts.join(chart), folder.join(name)).unwrap();
            paths.push(folder.join(name).display().to_string());
        }
        let job = ImportJob::start(library.clone(), paths.clone(), ImportMode::Move, true);
        wait(&job);
        assert_eq!(job.error(), None);
        assert_eq!((job.imported(), job.skipped(), job.done()), (2, 1, 3));
        let photos = library.photos().unwrap();
        assert_eq!(photos.len(), 2);
        assert!(photos.iter().all(|p| p.filename.starts_with("IMG_0001")));
        assert_ne!(photos[0].path, photos[1].path);
        assert!(photos.iter().all(|p| Path::new(&p.path).exists()));
        // Moved, not copied; the duplicate is left for the app to discard
        // with the rest of what it staged.
        assert!(!Path::new(&paths[0]).exists() && !Path::new(&paths[1]).exists());
        assert!(Path::new(&paths[2]).exists());
    }

    #[test]
    fn a_capture_date_that_isnt_a_date_never_becomes_a_path() {
        let file = Path::new("/nonexistent");
        assert_eq!(
            dated_folder("2023-04-22T10:00:00", file),
            Path::new("2023/2023-04-22")
        );
        for bad in ["../../etc/x", "2023/../..x", "abcd-ef-ghT", "2023-04-2/"] {
            assert_eq!(dated_folder(bad, file), Path::new("Undated"), "{bad}");
        }
    }

    #[test]
    fn days_are_counted_from_1970() {
        assert_eq!(civil_day(0), "1970-01-01");
        assert_eq!(civil_day(19_469), "2023-04-22");
        assert_eq!(civil_day(11_016), "2000-02-29");
    }
}
