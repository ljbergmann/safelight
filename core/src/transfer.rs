//! Stage 3 of the features: presets, and copying settings between photos.
//!
//! Presets are RAWmakase's preset library: its built-in presets and the
//! Lightroom XMP presets installed in the data folder's `xmp-presets`, which
//! the app fills from the Files app. A preset applies through
//! [`apply_to_photo`], the function the desktop's Develop and Library share,
//! with the decoded photo for its Auto settings.
//!
//! Copy and Paste carry the chosen [`SettingGroup`]s through
//! [`settings_groups::transfer`], which works out again for the target what
//! depends on the photo (white balance gains, profiles, Upright).

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_cancel::Cancel;
use rawmakase_catalog::catalog::{HistoryUpdate, PhotoId};
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::settings_groups::{
    self, GroupInclusion, GroupSelection, SettingGroup, Source, Target,
};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// A preset the Presets list shows.
#[derive(uniffi::Record)]
pub struct PresetInfo {
    /// What [`crate::LiveView::apply_preset`] takes.
    pub id: String,
    pub group: String,
    pub name: String,
}

/// A group Copy Settings offers, under its section's title.
#[derive(uniffi::Record)]
pub struct CopyGroup {
    pub index: u32,
    pub section: String,
    pub label: String,
    /// Included by default, as in Lightroom's Paste Settings.
    pub default_on: bool,
}

/// The settings copied last, for Paste: the recipe, the camera it was made
/// for, and the groups that paste. The [`Library`] keeps one.
pub(crate) struct Clipboard {
    recipe: Recipe,
    metadata: Metadata,
    groups: GroupSelection,
}

/// The library's copied settings.
pub(crate) type Copied = Mutex<Option<Clipboard>>;

fn clipboard(library: &Library) -> std::sync::MutexGuard<'_, Option<Clipboard>> {
    library
        .copied
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Every preset, built-in ones first, as the desktop's Presets panel lists them.
#[uniffi::export]
pub fn presets() -> Vec<PresetInfo> {
    rawmakase_interop::presets::load_library()
        .presets
        .into_iter()
        .map(|p| PresetInfo {
            id: p.id.clone(),
            group: p.group.clone(),
            name: rawmakase_interop::presets::display_name(&p.name),
        })
        .collect()
}

/// Copies Lightroom XMP presets into the preset library; returns how many.
#[uniffi::export]
pub fn import_presets(paths: Vec<String>) -> Result<u32, RenderError> {
    let dir = rawmakase_interop::presets::library_dirs()
        .into_iter()
        .next()
        .ok_or_else(|| RenderError::Failed {
            message: "No folder for presets".into(),
        })?
        .join("Imported");
    std::fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let mut copied = 0;
    for path in paths {
        let path = Path::new(&path);
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xmp"))
            && let Some(name) = path.file_name()
        {
            std::fs::copy(path, dir.join(name)).map_err(anyhow::Error::from)?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// The groups Copy Settings offers, by section, in Lightroom's order.
#[uniffi::export]
pub fn copy_groups() -> Vec<CopyGroup> {
    let default = GroupSelection::default();
    SettingGroup::SECTIONS
        .iter()
        .flat_map(|section| {
            section.groups.iter().map(|group| CopyGroup {
                index: index_of(*group),
                section: section.title.to_string(),
                label: group.label().to_string(),
                default_on: default.contains(*group),
            })
        })
        .collect()
}

fn index_of(group: SettingGroup) -> u32 {
    SettingGroup::ALL
        .iter()
        .position(|g| *g == group)
        .unwrap_or(0) as u32
}

fn selection(indices: &[u32]) -> GroupSelection {
    let mut groups = GroupSelection::none();
    for group in indices
        .iter()
        .filter_map(|i| SettingGroup::ALL.get(*i as usize))
    {
        groups.set(*group, GroupInclusion::Included);
    }
    groups
}

/// Keeps `recipe`, made for `metadata`, with `groups` for Paste.
pub(crate) fn copy(library: &Library, recipe: &Recipe, metadata: &Metadata, groups: &[u32]) {
    *clipboard(library) = Some(Clipboard {
        recipe: recipe.clone(),
        metadata: metadata.clone(),
        groups: selection(groups),
    });
}

/// The copied settings over `to`, a photo's recipe, for that photo; `None`
/// with nothing copied. Also returns what could not transfer as it was.
pub(crate) fn paste(
    library: &Library,
    to: &Recipe,
    metadata: &Metadata,
    profiles: &[Arc<rawmakase_model::camera_profiles::CameraProfile>],
) -> Option<(Recipe, Vec<String>)> {
    let copied = clipboard(library);
    let copied = copied.as_ref()?;
    let out = settings_groups::transfer(
        Source {
            recipe: &copied.recipe,
            metadata: &copied.metadata,
        },
        to,
        &copied.groups,
        Target { metadata, profiles },
    );
    Some((out.recipe, out.notes))
}

#[uniffi::export]
impl Library {
    /// Whether there is anything to paste.
    pub fn has_copied_settings(&self) -> bool {
        clipboard(self).is_some()
    }

    /// Pastes the copied settings onto photos `ids` of the catalog, each saved
    /// in one transaction with the rest; their History notices the change when
    /// they next open, as after a Library batch on the desktop. Returns notes
    /// on what didn't transfer.
    pub fn paste_settings(&self, ids: Vec<i64>) -> Result<Vec<String>, RenderError> {
        let photos = self.photos()?;
        let mut notes = Vec::new();
        for id in ids {
            let Some(photo) = photos.iter().find(|p| p.id == id) else {
                continue;
            };
            let path = Path::new(&photo.path);
            let raw = rawmakase_decode::photo::open(path)?;
            let profiles = rawmakase_decode::camera_look::with_default(
                path,
                &raw.metadata,
                rawmakase_model::camera_profiles::installed(&raw.metadata).0,
                Cancel::NEVER,
            );
            let record = self.catalog().edit_record(PhotoId(id))?;
            let resolved = rawmakase_interop::edits::resolve(
                &record,
                path,
                &raw.metadata,
                &profiles,
                &rawmakase_interop::raw_defaults::DevelopDefaults::default(),
            )?;
            let Some((recipe, more)) = paste(self, &resolved.recipe, &raw.metadata, &profiles)
            else {
                return Ok(notes);
            };
            recipe.validate()?;
            let identity = rawmakase_model::storage::Identity::read(path)?;
            self.catalog().save_edit(
                PhotoId(id),
                path,
                &identity,
                &recipe,
                &resolved.export,
                HistoryUpdate::Keep,
            )?;
            notes.extend(more.into_iter().map(|n| format!("{}: {n}", photo.filename)));
        }
        Ok(notes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::OpenEdit;
    use rawmakase_model::camera_data::Decode;
    use std::path::PathBuf;

    /// A catalog with two of RAWmakase's chart DNGs, from different cameras.
    fn library(dir: &Path) -> (Arc<Library>, Vec<(i64, PathBuf)>) {
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
        let photos = library
            .photos()
            .unwrap()
            .into_iter()
            .map(|p| (p.id, PathBuf::from(p.path)))
            .collect();
        (library, photos)
    }

    fn open(library: &Arc<Library>, (id, path): &(i64, PathBuf)) -> OpenEdit {
        let raw = rawmakase_decode::photo::open(path).unwrap();
        let profiles = rawmakase_decode::camera_look::with_default(
            path,
            &raw.metadata,
            rawmakase_model::camera_profiles::installed(&raw.metadata).0,
            Cancel::NEVER,
        );
        OpenEdit::load(library.clone(), *id, path.clone(), raw.metadata, &profiles).unwrap()
    }

    fn exposure_group() -> u32 {
        index_of(SettingGroup::Exposure)
    }

    #[test]
    fn pasting_copies_only_the_chosen_groups_as_one_step() {
        let dir = tempfile::tempdir().unwrap();
        let (library, photos) = library(dir.path());
        let mut first = open(&library, &photos[0]);
        first.set(crate::controls::Control::Param { index: 2 }, 0.7, true); // Exposure
        first.set(crate::controls::Control::Param { index: 3 }, 0.3, true); // Contrast
        copy(
            &library,
            first.recipe(),
            &first.metadata,
            &[exposure_group()],
        );

        let mut second = open(&library, &photos[1]);
        second.paste().unwrap();
        assert_eq!(second.recipe().exposure, 0.7);
        assert_eq!(second.recipe().contrast, 0., "Contrast wasn't chosen");
        assert_eq!(second.state().steps, ["Paste Settings"]);
    }

    #[test]
    fn pasting_onto_photos_in_the_grid_saves_each() {
        let dir = tempfile::tempdir().unwrap();
        let (library, photos) = library(dir.path());
        let mut first = open(&library, &photos[0]);
        first.set(crate::controls::Control::Param { index: 2 }, -0.4, true);
        copy(
            &library,
            first.recipe(),
            &first.metadata,
            &[exposure_group()],
        );
        drop(first);

        library.paste_settings(vec![photos[1].0]).unwrap();
        let reopened = open(&library, &photos[1]);
        assert_eq!(reopened.recipe().exposure, -0.4);
        // The photo copied from was never saved: untouched.
        assert_eq!(open(&library, &photos[0]).recipe().exposure, 0.);
    }

    #[test]
    fn a_built_in_preset_applies_as_one_named_step() {
        let dir = tempfile::tempdir().unwrap();
        let (library, photos) = library(dir.path());
        let mut edit = open(&library, &photos[0]);
        let image = rawmakase_decode::photo::open(&photos[0].1)
            .unwrap()
            .develop(Decode::Half, Cancel::NEVER)
            .unwrap();
        let listed = presets();
        let info = listed
            .iter()
            .find(|p| p.id.starts_with("builtin:"))
            .expect("built-in presets");
        let preset = rawmakase_interop::presets::find_preset(&info.id).unwrap();
        let before = edit.recipe().clone();
        edit.apply_preset(&preset, &image).unwrap();
        assert_ne!(*edit.recipe(), before, "{} changed nothing", info.name);
        assert_eq!(edit.state().steps, [format!("Preset {}", info.name)]);
    }

    #[test]
    fn copy_settings_lists_every_group_once_under_its_section() {
        let groups = copy_groups();
        assert_eq!(groups.len(), SettingGroup::ALL.len());
        assert!(groups.iter().any(|g| g.label == "Exposure" && g.default_on));
        // Spots and masks stay with the photo by default, as in Lightroom.
        assert!(groups.iter().any(|g| !g.default_on));
    }
}
