//! Stage 3 of the features: presets, and copying settings between photos.
//!
//! Presets are RAWmakase's preset library: its built-in presets and the
//! Lightroom XMP presets installed in the data folder's `xmp-presets`, which
//! the app fills from the Files app. A preset applies through
//! [`apply_to_photo`], the function the desktop's Develop and Library share,
//! with the decoded photo for its Auto settings.
//!
//! Presets made here (New, Update, Rename, Delete) go through
//! [`UserPresets`], the desktop's own rules: only presets made in RAWmakase
//! or Safelight, or imported into the library, can change. After a preset
//! that offers one is applied, its Amount scales it ([`PresetAmount`]).
//!
//! Copy and Paste carry the chosen [`SettingGroup`]s through
//! [`settings_groups::transfer`], which works out again for the target what
//! depends on the photo (white balance gains, profiles, Upright).

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_cancel::Cancel;
use rawmakase_catalog::catalog::{HistoryUpdate, PhotoId};
use rawmakase_interop::presets::amount::PresetAmount;
use rawmakase_interop::presets::user::UserPresets;
use rawmakase_interop::xmp::Preset;
use rawmakase_interop::xmp::preset_write::PresetInfo as NewPreset;
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::settings_groups::{
    self, GroupInclusion, GroupSelection, SettingGroup, Source, Target,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// A preset the Presets list shows.
#[derive(uniffi::Record)]
pub struct PresetInfo {
    /// What [`crate::LiveView::apply_preset`] takes.
    pub id: String,
    pub group: String,
    pub name: String,
    /// Made here or imported, so it may be updated, renamed and deleted.
    pub managed: bool,
    /// Starred, for the Favorites list, as on the desktop.
    pub favorite: bool,
}

/// The Amount slider of the preset just applied, while nothing else has
/// changed the photo.
#[derive(uniffi::Record)]
pub struct PresetAmountState {
    pub name: String,
    /// 0–2, 1 being the preset as applied.
    pub amount: f32,
}

/// Lightroom's preset Amount while it shows, as the desktop's Develop keeps
/// it (its `AmountSession`, which stays in its app crate): every position is
/// computed from the settings kept when the preset was applied, so dragging
/// back and forth never drifts. The iPad has no Upright analysis landing
/// later, so any difference from what it last set ends it.
pub(crate) struct AmountShown {
    pub(crate) name: String,
    pub(crate) amount: f32,
    pub(crate) scale: PresetAmount,
    /// The settings it last set, to notice any other change.
    pub(crate) shown: Recipe,
}

impl AmountShown {
    /// The Amount for `preset`, applied over `before` as `full`; `None` when
    /// the preset offers none or changes something that doesn't scale.
    pub(crate) fn new(preset: &Preset, before: Recipe, full: Recipe) -> Option<Self> {
        let scale = PresetAmount::new(preset, before, full.clone()).ok()?;
        Some(Self {
            name: rawmakase_interop::presets::display_name(&preset.name),
            amount: 1.,
            scale,
            shown: full,
        })
    }

    pub(crate) fn state(&self, current: &Recipe) -> Option<PresetAmountState> {
        (self.shown == *current).then(|| PresetAmountState {
            name: self.name.clone(),
            amount: self.amount,
        })
    }
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

/// The settings of the photo left last in the editor, for Paste from
/// Previous: every group, as the desktop's.
pub(crate) fn remember_previous(library: &Library, recipe: &Recipe, metadata: &Metadata) {
    *library
        .previous
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(Clipboard {
        recipe: recipe.clone(),
        metadata: metadata.clone(),
        groups: GroupSelection::default(),
    });
}

/// The previous photo's settings over `to`, as the desktop's Paste from
/// Previous: the default groups. `None` with no previous photo, or when it
/// is this one.
pub(crate) fn paste_previous(
    library: &Library,
    to: &Recipe,
    metadata: &Metadata,
    profiles: &[Arc<rawmakase_model::camera_profiles::CameraProfile>],
) -> Option<(Recipe, Vec<String>)> {
    let previous = library
        .previous
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let previous = previous.as_ref()?;
    let out = settings_groups::transfer(
        Source {
            recipe: &previous.recipe,
            metadata: &previous.metadata,
        },
        to,
        &previous.groups,
        Target { metadata, profiles },
    );
    Some((out.recipe, out.notes))
}

fn clipboard(library: &Library) -> std::sync::MutexGuard<'_, Option<Clipboard>> {
    library
        .copied
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Every preset, built-in ones first, as the desktop's Presets panel lists them.
#[uniffi::export]
pub fn presets() -> Vec<PresetInfo> {
    let user = UserPresets::default();
    let favorites = rawmakase_interop::presets::load_favorites();
    rawmakase_interop::presets::load_library()
        .presets
        .into_iter()
        .map(|p| PresetInfo {
            name: rawmakase_interop::presets::display_name(&p.name),
            managed: user.manages(&p),
            group: p.group.clone(),
            favorite: favorites.contains(&p.id),
            id: p.id.clone(),
        })
        .collect()
}

/// Stars preset `id` as a favorite, or takes its star away, in the
/// favorites the desktop keeps too.
#[uniffi::export]
pub fn set_preset_favorite(id: String, favorite: bool) -> Result<(), RenderError> {
    let mut favorites = rawmakase_interop::presets::load_favorites();
    if favorite {
        favorites.insert(id);
    } else {
        favorites.remove(&id);
    }
    rawmakase_interop::presets::save_favorites(&favorites)?;
    Ok(())
}

/// The groups a new preset can go in: "User Presets" and the groups of
/// presets made here.
#[uniffi::export]
pub fn preset_groups() -> Vec<String> {
    let user = UserPresets::default();
    let mut groups = vec!["User Presets".to_string()];
    for p in rawmakase_interop::presets::load_library().presets {
        if user.owns(&p) && !p.group.is_empty() && !groups.contains(&p.group) {
            groups.push(p.group);
        }
    }
    groups
}

/// The preset `id` names, when it is still there.
pub(crate) fn find(id: &str) -> Result<Preset, RenderError> {
    rawmakase_interop::presets::find_preset(id).ok_or_else(|| RenderError::Failed {
        message: "The preset is gone".into(),
    })
}

/// Renames a preset made here or imported, keeping its settings.
#[uniffi::export]
pub fn rename_preset(id: String, name: String) -> Result<(), RenderError> {
    UserPresets::default().rename(&find(&id)?, &name)?;
    Ok(())
}

/// Deletes a preset made here or imported.
#[uniffi::export]
pub fn delete_preset(id: String) -> Result<(), RenderError> {
    UserPresets::default().delete(&find(&id)?)?;
    Ok(())
}

/// Saves `recipe`'s `groups` (indices from [`copy_groups`]), edited on a
/// photo of `metadata`, as a new preset `name` in `group`, as the desktop's
/// New Develop Preset does: never with AI Denoise, which presets don't carry.
pub(crate) fn create_preset(
    user: &UserPresets,
    recipe: &Recipe,
    metadata: &Metadata,
    name: &str,
    group: &str,
    groups: &[u32],
) -> anyhow::Result<PathBuf> {
    let mut groups = selection(groups);
    groups.set(SettingGroup::AiDenoise, GroupInclusion::Excluded);
    user.create(recipe, metadata, &NewPreset::new(name, group), &groups)
}

/// Lightroom's Update with Current Settings for `preset`.
pub(crate) fn update_preset(
    user: &UserPresets,
    preset: &Preset,
    recipe: &Recipe,
    metadata: &Metadata,
) -> anyhow::Result<()> {
    user.update(preset, recipe, metadata)?;
    Ok(())
}

/// Imports Lightroom XMP presets from `paths`, files or folders, as the
/// desktop's Import Presets does: into the preset library's Imported folder,
/// keeping their folders, skipping presets already there, and taking the
/// look profiles packs ship beside their presets into the profile library.
#[uniffi::export]
pub fn import_presets(paths: Vec<String>) -> crate::profile::ImportSummary {
    let picks: Vec<std::path::PathBuf> = paths.iter().map(Into::into).collect();
    let summary = crate::from_desktop::bulk_import::run(
        crate::from_desktop::bulk_import::ImportKind::Presets,
        &picks,
        &crate::from_desktop::bulk_import::ImportProgress::default(),
    );
    (&summary).into()
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

    /// The user library's folders, in `dir`.
    fn user_presets(dir: &Path) -> UserPresets {
        UserPresets {
            dir: dir.join("xmp-presets/User Presets"),
            imported: dir.join("xmp-presets/Imported"),
            backups: dir.join("preset-backups"),
        }
    }

    fn read(path: &str) -> Preset {
        rawmakase_interop::xmp::parse(Path::new(path), &std::fs::read_to_string(path).unwrap())
            .unwrap()
    }

    #[test]
    fn a_preset_made_here_applies_updates_renames_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let (library, photos) = library(dir.path());
        let user = user_presets(dir.path());
        let mut first = open(&library, &photos[0]);
        first.set(crate::controls::Control::Param { index: 2 }, 0.6, true);
        first.set(crate::controls::Control::Param { index: 3 }, 0.4, true);
        let id = first
            .create_preset(&user, "Bright", "Mine", &[exposure_group()])
            .unwrap();
        let made = read(&id);
        assert!(user.owns(&made) && user.manages(&made));
        assert_eq!(
            (made.name.as_str(), made.group.as_str()),
            ("Bright", "Mine")
        );
        assert!(
            first
                .create_preset(&user, "Bright", "Mine", &[exposure_group()])
                .is_err(),
            "a second preset of the same name is refused"
        );
        assert!(first.create_preset(&user, "Empty", "Mine", &[]).is_err());

        // On another photo it sets only what it holds.
        let image = rawmakase_decode::photo::open(&photos[1].1)
            .unwrap()
            .develop(Decode::Half, Cancel::NEVER)
            .unwrap();
        let mut second = open(&library, &photos[1]);
        second.apply_preset(&made, &image).unwrap();
        assert_eq!(second.recipe().exposure, 0.6);
        assert_eq!(second.recipe().contrast, 0.);

        // Update with Current Settings keeps its groups and takes the values.
        first.set(crate::controls::Control::Param { index: 2 }, -0.3, true);
        first.update_preset(&user, &made).unwrap();
        let updated = read(&id);
        assert_eq!(updated.settings.get("UUID"), made.settings.get("UUID"));
        let mut third = open(&library, &photos[1]);
        third.apply_preset(&updated, &image).unwrap();
        assert_eq!(third.recipe().exposure, -0.3);
        assert_eq!(third.recipe().contrast, 0., "still only Exposure");

        let renamed = user.rename(&updated, "Dim").unwrap();
        let renamed = read(&renamed.display().to_string());
        assert_eq!(renamed.name, "Dim");
        assert!(!Path::new(&id).exists());
        user.delete(&renamed).unwrap();
        assert!(!renamed.path.exists());

        // A built-in preset is never changed.
        let builtin = rawmakase_interop::presets::builtin::presets().0.remove(0);
        assert!(!user.manages(&builtin));
        assert!(first.update_preset(&user, &builtin).is_err());
    }

    #[test]
    fn preset_amount_scales_from_the_settings_kept_once_and_ends_with_another_edit() {
        let dir = tempfile::tempdir().unwrap();
        let (library, photos) = library(dir.path());
        let image = rawmakase_decode::photo::open(&photos[0].1)
            .unwrap()
            .develop(Decode::Half, Cancel::NEVER)
            .unwrap();
        let mut edit = open(&library, &photos[0]);
        let before = edit.recipe().clone();
        let preset = rawmakase_interop::presets::builtin::presets()
            .0
            .into_iter()
            .find(|p| {
                let full = rawmakase_interop::presets::apply_to_photo(
                    p,
                    &before,
                    &edit.metadata,
                    &edit.profiles,
                    None,
                )
                .unwrap()
                .recipe;
                full != before && AmountShown::new(p, before.clone(), full).is_some()
            })
            .expect("a built-in preset with an Amount");
        edit.apply_preset(&preset, &image).unwrap();
        let full = edit.recipe().clone();
        let shown = edit.state().preset_amount.expect("the Amount shows");
        assert_eq!(shown.amount, 1.);

        // A drag through 0 and back to 50% is one step, and 0 is the photo as before.
        assert!(edit.set_preset_amount(0., false));
        assert_eq!(*edit.recipe(), before);
        edit.set_preset_amount(0.5, true);
        let steps = edit.state().steps;
        assert_eq!(steps.len(), 2, "{steps:?}");
        assert_eq!(steps[1], "Preset Amount 50");
        assert_eq!(edit.state().preset_amount.unwrap().amount, 0.5);
        edit.set_preset_amount(1., true);
        assert_eq!(*edit.recipe(), full, "back to 100% without drift");

        // Any other edit ends it.
        edit.set(crate::controls::Control::Param { index: 2 }, 0.2, true);
        assert!(edit.state().preset_amount.is_none());
        assert!(!edit.set_preset_amount(0., true));
        assert_eq!(edit.recipe().exposure, 0.2);
    }
}
