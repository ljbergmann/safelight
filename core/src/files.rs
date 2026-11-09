//! Deleting, renaming and moving photos and folders, through RAWmakase's
//! file operations ([`file_ops`]): planned, then run with every disk step
//! journalled, so no original is lost or overwritten and the catalog follows
//! the files only once they are proven where they went. An operation a crash
//! cut short is decided when the library next opens.
//!
//! iPadOS gives an app no system Trash, so the library keeps its own:
//! Recently Deleted, a folder in the app's storage that a deleted photo's
//! files are renamed into, as Photos and Lightroom keep theirs. Emptying it
//! is the one step that deletes for good.

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_cancel::Cancel;
use rawmakase_catalog::catalog::file_ops::{
    self, FileOp, ItemResult, Trash, TrashError, TrashReceipt, TrashSupport,
};
use rawmakase_catalog::catalog::{Catalog, PhotoId, RootId};
use std::path::{Path, PathBuf};

/// The library's Trash: deleted files are renamed into a folder of their
/// own under `dir`, never across volumes.
pub(crate) struct RecentlyDeleted {
    pub(crate) dir: PathBuf,
}

impl Trash for RecentlyDeleted {
    fn trash(&self, path: &Path) -> Result<TrashReceipt, TrashError> {
        let name = path
            .file_name()
            .ok_or_else(|| TrashError::Failed(format!("{} has no name", path.display())))?;
        // A folder per deletion, so two files of one name never meet.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let folder = (0..)
            .map(|n| self.dir.join(format!("{stamp}-{n}")))
            .find(|f| !f.exists())
            .expect("some name is free");
        std::fs::create_dir_all(&folder).map_err(|e| TrashError::Failed(e.to_string()))?;
        let location = folder.join(name);
        std::fs::rename(path, &location).map_err(|e| TrashError::Failed(e.to_string()))?;
        Ok(TrashReceipt { location })
    }

    fn supports(&self, _path: &Path) -> TrashSupport {
        TrashSupport::Yes
    }
}

/// What a file operation did, for the app to say.
#[derive(uniffi::Record, Debug, Default, PartialEq)]
pub struct FileChange {
    /// Photos it changed (removed, deleted, renamed or moved), virtual
    /// copies included.
    pub changed: u32,
    /// What it left alone, and why.
    pub problems: Vec<String>,
}

/// A folder of the library, by its path under the photo folder.
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct FolderInfo {
    /// "2026/2026-11-08"; what moving and renaming take.
    pub path: String,
    pub photos: u32,
}

impl Library {
    pub(crate) fn recently_deleted(&self) -> RecentlyDeleted {
        RecentlyDeleted {
            dir: self.data_dir().join("Recently Deleted"),
        }
    }

    /// The root of the library's photo folder.
    fn photo_root(catalog: &Catalog, photos: &Path) -> Result<RootId, RenderError> {
        let here = photos
            .canonicalize()
            .unwrap_or_else(|_| photos.to_path_buf());
        catalog
            .roots()?
            .into_iter()
            .find(|(_, original, location)| {
                let path = PathBuf::from(location.as_deref().unwrap_or(original));
                path.canonicalize().unwrap_or(path) == here
            })
            .map(|(id, _, _)| id)
            .ok_or_else(|| RenderError::Failed {
                message: "The photo folder isn't in the catalog yet".into(),
            })
    }

    /// Plans and runs `op`, as the desktop runs one once it is confirmed:
    /// the app asks before calling.
    fn run(&self, op: impl FnOnce(RootId) -> FileOp) -> Result<FileChange, RenderError> {
        let trash = self.recently_deleted();
        let mut catalog = self.catalog();
        let root = Self::photo_root(&catalog, Path::new(&self.photos_folder()))?;
        let failed = |e: file_ops::FileOpError| RenderError::Failed {
            message: e.to_string(),
        };
        let plan = file_ops::plan_with_trash(&catalog, op(root), &trash).map_err(failed)?;
        let mut change = FileChange {
            problems: plan
                .skipped()
                .iter()
                .map(|s| format!("{:?}", s.reason))
                .collect(),
            ..FileChange::default()
        };
        let outcome = file_ops::execute(&mut catalog, plan.confirm(), &trash, Cancel::NEVER)
            .map_err(failed)?;
        for item in outcome.items {
            match item.result {
                ItemResult::Removed
                | ItemResult::Trashed
                | ItemResult::Moved { .. }
                | ItemResult::Renamed { .. }
                | ItemResult::FolderRenamed
                | ItemResult::FolderMoved
                | ItemResult::Created { .. } => change.changed += item.photos.len() as u32,
                ItemResult::Unchanged(Some(why)) | ItemResult::NeedsRecovery(why) => {
                    change.problems.push(why)
                }
                ItemResult::Unchanged(None) | ItemResult::Kept | ItemResult::Interrupted => {}
            }
        }
        change.problems.extend(outcome.stopped);
        Ok(change)
    }

    /// Decides what a crash left unfinished, as the desktop does when it
    /// opens a catalog. What can't be decided is left as it was.
    pub(crate) fn recover_file_operations(&self) -> Result<(), RenderError> {
        let mut catalog = self.catalog();
        if !file_ops::unfinished_here(&catalog)? {
            return Ok(());
        }
        file_ops::recover(&mut catalog, &self.recently_deleted()).map_err(|e| {
            RenderError::Failed {
                message: e.to_string(),
            }
        })?;
        Ok(())
    }
}

fn ids(ids: Vec<i64>) -> Vec<PhotoId> {
    ids.into_iter().map(PhotoId).collect()
}

#[uniffi::export]
impl Library {
    /// Takes photos out of the catalog with their edits; their files stay.
    pub fn remove_from_catalog(&self, photos: Vec<i64>) -> Result<FileChange, RenderError> {
        self.run(|_| FileOp::RemoveFromCatalog {
            photos: ids(photos),
        })
    }

    /// Moves photos' files into Recently Deleted and takes them out of the
    /// catalog, virtual copies with their master.
    pub fn delete_photos(&self, photos: Vec<i64>) -> Result<FileChange, RenderError> {
        self.run(|_| FileOp::TrashPhotos {
            photos: ids(photos),
        })
    }

    /// Gives a photo's file the name `stem`, its extension kept.
    pub fn rename_photo(&self, photo: i64, stem: String) -> Result<FileChange, RenderError> {
        if let Some(problem) = file_ops::photo_name_problem(stem.trim()) {
            return Err(RenderError::Failed {
                message: format!("{problem:?}"),
            });
        }
        self.run(|_| FileOp::RenamePhotos {
            renames: vec![(PhotoId(photo), stem.trim().to_string())],
        })
    }

    /// The library's folders that hold photos, by path.
    pub fn folders(&self) -> Result<Vec<FolderInfo>, RenderError> {
        let mut folders: Vec<FolderInfo> = self
            .catalog()
            .folders()?
            .into_iter()
            .filter(|f| !f.relative.is_empty())
            .map(|f| FolderInfo {
                path: f.relative,
                photos: f.count as u32,
            })
            .collect();
        folders.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(folders)
    }

    /// Moves photos into folder `folder` (a path from [`Self::folders`]).
    pub fn move_photos(&self, photos: Vec<i64>, folder: String) -> Result<FileChange, RenderError> {
        self.run(|root| FileOp::MovePhotos {
            photos: ids(photos),
            root,
            to: folder,
        })
    }

    /// Makes folder `name` in `parent` (a folder's path, or empty for the
    /// photo folder itself) and moves `photos` into it.
    pub fn new_folder(
        &self,
        parent: String,
        name: String,
        photos: Vec<i64>,
    ) -> Result<FileChange, RenderError> {
        if let Some(problem) = file_ops::folder_name_problem(name.trim()) {
            return Err(RenderError::Failed {
                message: problem.into(),
            });
        }
        self.run(|root| FileOp::CreateFolder {
            root,
            parent,
            name: name.trim().to_string(),
            then_move: ids(photos),
        })
    }

    /// Renames folder `folder` to `name`; its photos follow.
    pub fn rename_folder(&self, folder: String, name: String) -> Result<FileChange, RenderError> {
        if let Some(problem) = file_ops::folder_name_problem(name.trim()) {
            return Err(RenderError::Failed {
                message: problem.into(),
            });
        }
        self.run(|root| FileOp::RenameFolder {
            root,
            folder,
            name: name.trim().to_string(),
        })
    }

    /// How many deleted files Recently Deleted holds, and their bytes.
    pub fn recently_deleted_size(&self) -> Vec<u64> {
        let (mut files, mut bytes) = (0, 0);
        let mut stack = vec![self.recently_deleted().dir];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                match entry.metadata() {
                    Ok(m) if m.is_dir() => stack.push(entry.path()),
                    Ok(m) => {
                        files += 1;
                        bytes += m.len();
                    }
                    Err(_) => {}
                }
            }
        }
        vec![files, bytes]
    }

    /// Deletes what Recently Deleted holds, for good. The app asks first.
    pub fn empty_recently_deleted(&self) -> Result<(), RenderError> {
        let dir = self.recently_deleted().dir;
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(anyhow::Error::from(e).into())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Two chart DNGs in a dated folder of a new library.
    fn library(dir: &Path) -> (Arc<Library>, Vec<i64>) {
        let library = Library::open(
            dir.join("data").display().to_string(),
            dir.join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("2026/2026-11-08");
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

    /// Albums sit in folders and hold photos; the list says which is which
    /// and where each is; deleting a folder takes its albums, never photos.
    #[test]
    fn albums_and_their_folders() {
        let dir = tempfile::tempdir().unwrap();
        let (library, ids) = library(dir.path());
        let trips = library.create_album("Trips".into(), None, true).unwrap();
        let japan = library
            .create_album("Japan".into(), Some(trips), false)
            .unwrap();
        library.change_album(japan, ids.clone(), true).unwrap();
        library.change_album(japan, vec![ids[0]], false).unwrap();
        let listed = library.collections().unwrap();
        let folder = listed.iter().find(|c| c.id == trips).unwrap();
        let album = listed.iter().find(|c| c.id == japan).unwrap();
        assert!(folder.folder && folder.photos.is_empty());
        assert!(!album.folder);
        assert_eq!(album.parent, Some(trips));
        assert_eq!(album.photos, vec![ids[1]]);
        library.rename_album(japan, "Kyoto".into()).unwrap();
        library.move_album(japan, None).unwrap();
        let moved = library.collections().unwrap();
        let album = moved.iter().find(|c| c.id == japan).unwrap();
        assert_eq!((album.name.as_str(), album.parent), ("Kyoto", None));
        library.move_album(japan, Some(trips)).unwrap();
        library.delete_album(trips).unwrap();
        assert!(library.collections().unwrap().iter().all(|c| c.quick));
        assert_eq!(library.photos().unwrap().len(), 2);
    }

    fn path_of(library: &Library, id: i64) -> Option<PathBuf> {
        library
            .photos()
            .unwrap()
            .into_iter()
            .find(|p| p.id == id)
            .map(|p| PathBuf::from(p.path))
    }

    #[test]
    fn deleting_moves_the_files_to_recently_deleted_until_it_is_emptied() {
        let dir = tempfile::tempdir().unwrap();
        let (library, ids) = library(dir.path());
        let file = path_of(&library, ids[0]).unwrap();
        let copy = library.create_virtual_copy(ids[0]).unwrap();
        let change = library.delete_photos(vec![ids[0]]).unwrap();
        assert_eq!(change.changed, 2, "the master took its copy: {change:?}");
        assert!(!file.exists());
        assert!(path_of(&library, copy).is_none());
        assert_eq!(library.photos().unwrap().len(), 1);
        assert_eq!(library.recently_deleted_size()[0], 1);
        library.empty_recently_deleted().unwrap();
        assert_eq!(library.recently_deleted_size(), [0, 0]);
    }

    #[test]
    fn removing_from_the_catalog_leaves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (library, ids) = library(dir.path());
        let file = path_of(&library, ids[1]).unwrap();
        assert_eq!(
            library.remove_from_catalog(vec![ids[1]]).unwrap().changed,
            1
        );
        assert!(file.exists());
        assert_eq!(library.photos().unwrap().len(), 1);
    }

    #[test]
    fn renaming_and_moving_keep_the_photo_and_its_edit() {
        let dir = tempfile::tempdir().unwrap();
        let (library, ids) = library(dir.path());
        library.set_rating(ids[0], 4).unwrap();
        library.rename_photo(ids[0], "Harbour".into()).unwrap();
        let renamed = path_of(&library, ids[0]).unwrap();
        assert_eq!(renamed.file_name().unwrap(), "Harbour.dng");
        assert!(renamed.exists());
        assert!(library.rename_photo(ids[0], "a/b".into()).is_err());

        let folders = library.folders().unwrap();
        assert_eq!(folders[0].path, "2026/2026-11-08");
        library
            .new_folder("2026".into(), "Best".into(), vec![ids[0]])
            .unwrap();
        let moved = path_of(&library, ids[0]).unwrap();
        assert!(
            moved.ends_with("2026/Best/Harbour.dng"),
            "{}",
            moved.display()
        );
        assert!(moved.exists() && !renamed.exists());
        library
            .move_photos(vec![ids[0]], "2026/2026-11-08".into())
            .unwrap();
        assert!(
            path_of(&library, ids[0])
                .unwrap()
                .ends_with("2026/2026-11-08/Harbour.dng")
        );

        library
            .rename_folder("2026/2026-11-08".into(), "Harbour day".into())
            .unwrap();
        let photos = library.photos().unwrap();
        assert!(photos.iter().all(|p| p.path.contains("2026/Harbour day/")));
        assert!(photos.iter().all(|p| Path::new(&p.path).exists()));
        assert_eq!(photos.iter().find(|p| p.id == ids[0]).unwrap().rating, 4);
    }
}
