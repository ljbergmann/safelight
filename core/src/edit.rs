//! Stage 5: the open photo's edit, with History, saved to the catalog.
//!
//! The edit is RAWmakase's own [`EditSession`], as the desktop's Develop keeps
//! it: every change goes through [`change_setting`], the model's one way to
//! change a develop setting (it recomputes the white balance when Temp or Tint
//! move), inside an editor frame, so a slider drag becomes one History step
//! named as the desktop names it. The edit and its History load from the
//! catalog the way the desktop resolves them, and save back in one transaction.

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_catalog::catalog::{HistoryUpdate, PhotoId};
use rawmakase_edit_session::history::Step;
use rawmakase_edit_session::{EditSession, Gesture};
use rawmakase_interop::export_settings::ExportOptions;
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::edit::change_setting;
use rawmakase_model::model::params::ParameterId;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::storage::Identity;
use std::path::PathBuf;
use std::sync::Arc;

/// The Basic sliders the iPad shows.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Exposure,
    Contrast,
    Highlights,
    Shadows,
    Temperature,
    Tint,
}

impl Param {
    pub const ALL: [Param; 6] = [
        Param::Exposure,
        Param::Contrast,
        Param::Highlights,
        Param::Shadows,
        Param::Temperature,
        Param::Tint,
    ];

    /// The model's setting for this photo: a rendered file's Temp and Tint
    /// are relative, a RAW's in Kelvin and tint units.
    fn id(self, photo: &Metadata) -> ParameterId {
        match self {
            Param::Exposure => ParameterId::Exposure,
            Param::Contrast => ParameterId::Contrast,
            Param::Highlights => ParameterId::Highlights,
            Param::Shadows => ParameterId::Shadows,
            Param::Temperature => ParameterId::Temperature,
            Param::Tint => ParameterId::Tint,
        }
        .for_photo(Some(photo))
    }
}

/// A slider as the model describes it, with its current value.
#[derive(uniffi::Record)]
pub struct ParamValue {
    pub param: Param,
    pub label: String,
    /// In the recipe's units, within `min`..`max` (the slider's range).
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// What dragging snaps to (0.05 EV for Exposure); 0 moves freely.
    pub step: f32,
    /// The value as the desktop shows it ("+0.35", "5200").
    pub text: String,
}

/// The edit as the editor shows it.
#[derive(uniffi::Record)]
pub struct EditState {
    pub values: Vec<ParamValue>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// History, oldest first, as "Exposure +0.35"; `applied` of them are applied.
    pub steps: Vec<String>,
    pub applied: u32,
    /// Changes not saved to the catalog yet.
    pub needs_save: bool,
    /// Why the last save failed; the edit stays unsaved.
    pub save_error: Option<String>,
}

/// The open photo's edit: which photo, what it is, and its session.
pub(crate) struct OpenEdit {
    pub(crate) library: Arc<Library>,
    pub(crate) id: i64,
    pub(crate) path: PathBuf,
    pub(crate) metadata: Metadata,
    pub(crate) export: ExportOptions,
    pub(crate) session: EditSession,
}

impl OpenEdit {
    /// Loads photo `id`'s edit and History from the catalog, resolved for
    /// `metadata` with `profiles` as the desktop resolves a photo it opens.
    pub(crate) fn load(
        library: Arc<Library>,
        id: i64,
        path: PathBuf,
        metadata: Metadata,
        profiles: &[Arc<rawmakase_model::camera_profiles::CameraProfile>],
    ) -> anyhow::Result<Self> {
        let (record, history) = {
            let catalog = library.catalog();
            (
                catalog.edit_record(PhotoId(id))?,
                catalog.load_history(PhotoId(id))?,
            )
        };
        let resolved = rawmakase_interop::edits::resolve(
            &record,
            &path,
            &metadata,
            profiles,
            &rawmakase_interop::raw_defaults::DevelopDefaults::default(),
        )?;
        resolved.recipe.validate()?;
        let mut session = EditSession::default();
        session.replace(resolved.recipe);
        if let Some(saved) = history {
            session.restore_history(saved);
        }
        Ok(Self {
            library,
            id,
            path,
            metadata,
            export: resolved.export,
            session,
        })
    }

    pub(crate) fn recipe(&self) -> &Recipe {
        self.session.recipe()
    }

    /// Sets `param` to `value`, clamped to its slider's range, as one editor
    /// frame; `done` ends the drag, which becomes one History step.
    pub(crate) fn set(&mut self, param: Param, value: f32, done: bool) -> bool {
        let id = param.id(&self.metadata);
        let d = id.descriptor();
        let value = value.clamp(*d.interactive.start(), *d.interactive.end());
        let frame = self.session.begin();
        change_setting(self.session.recipe_mut(), id, Some(&self.metadata), |r| {
            *id.value_mut(r) = value;
        });
        // Named on every frame, not only the last: a drag a save has to finish
        // (the app leaving mid-drag) keeps the name the slider gives it.
        let shown = *id.value_mut(self.session.recipe_mut());
        self.session
            .name_next_step(Step::new(d.label, id.text(shown)));
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        let outcome = self.session.finish(frame, gesture);
        outcome == rawmakase_edit_session::FrameOutcome::Edited
    }

    /// Moves History one step back (`by` −1) or forward (+1).
    pub(crate) fn step(&mut self, by: i32) -> bool {
        self.session.finish_gesture();
        let (_, applied) = self.session.history().steps();
        let Some(target) = applied.checked_add_signed(by as isize) else {
            return false;
        };
        self.session.jump(target)
    }

    pub(crate) fn state(&self) -> EditState {
        let mut recipe = self.session.recipe().clone();
        let values = Param::ALL
            .into_iter()
            .map(|param| {
                let id = param.id(&self.metadata);
                let d = id.descriptor();
                let value = *id.value_mut(&mut recipe);
                ParamValue {
                    param,
                    label: d.label.to_string(),
                    value,
                    min: *d.interactive.start(),
                    max: *d.interactive.end(),
                    step: d.drag_step.unwrap_or(0.),
                    text: id.text(value),
                }
            })
            .collect();
        let (steps, applied) = self.session.history().steps();
        EditState {
            values,
            can_undo: applied > 0,
            can_redo: applied < steps.len(),
            steps: steps
                .iter()
                .map(|s| format!("{} {}", s.name, s.value).trim().to_string())
                .collect(),
            applied: applied as u32,
            needs_save: self.session.save_state().needs_save(),
            save_error: self.session.save_state().message().map(str::to_string),
        }
    }

    /// Saves the edit and its History to the catalog in one transaction, when
    /// there is anything to save. A drag still held is finished first, as one
    /// step. On failure the edit stays unsaved and says why.
    pub(crate) fn save(&mut self) -> Result<(), RenderError> {
        self.session.finish_gesture();
        if !self.session.save_state().needs_save() {
            return Ok(());
        }
        let result = (|| -> anyhow::Result<()> {
            let identity = Identity::read(&self.path)?;
            let recipe = self.session.recipe();
            let history = self.session.history().saved(recipe);
            self.library.catalog().save_edit(
                PhotoId(self.id),
                &self.path,
                &identity,
                recipe,
                &self.export,
                HistoryUpdate::Replace(&history),
            )
        })();
        match result {
            Ok(()) => {
                self.session.save_state_mut().saved();
                Ok(())
            }
            Err(e) => {
                let message = format!("{e:#}");
                self.session.save_state_mut().failed(message.clone());
                Err(RenderError::Failed { message })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawmakase_cancel::Cancel;
    use std::path::Path;

    /// A catalog with one of RAWmakase's synthetic chart DNGs, and the photo's id.
    fn library(dir: &Path) -> (Arc<Library>, i64, PathBuf) {
        let library = Library::open(
            dir.join("data").display().to_string(),
            dir.join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        let chart = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        let path = day.join("chart.dng");
        std::fs::copy(chart, &path).unwrap();
        library.add_new_photos().unwrap();
        let id = library.photos().unwrap()[0].id;
        (library, id, path)
    }

    fn open(library: &Arc<Library>, id: i64, path: &Path) -> OpenEdit {
        let raw = rawmakase_decode::photo::open(path).unwrap();
        let profiles = rawmakase_decode::camera_look::with_default(
            path,
            &raw.metadata,
            rawmakase_model::camera_profiles::installed(&raw.metadata).0,
            Cancel::NEVER,
        );
        OpenEdit::load(
            library.clone(),
            id,
            path.to_path_buf(),
            raw.metadata,
            &profiles,
        )
        .unwrap()
    }

    fn exposure(edit: &OpenEdit) -> f32 {
        edit.state().values[0].value
    }

    #[test]
    fn a_drag_is_one_named_step_and_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        assert_eq!(edit.state().steps.len(), 0);

        edit.set(Param::Exposure, 0.3, false);
        edit.set(Param::Exposure, 0.4, false);
        edit.set(Param::Exposure, 0.5, true);
        let state = edit.state();
        assert_eq!(state.steps, ["Exposure +0.50"]);
        assert_eq!(state.applied, 1);
        assert!(state.needs_save);
        edit.save().unwrap();
        assert!(!edit.state().needs_save);
        drop(edit);

        let reopened = open(&library, id, &path);
        assert_eq!(exposure(&reopened), 0.5);
        let state = reopened.state();
        assert_eq!(state.steps, ["Exposure +0.50"]);
        assert_eq!(state.applied, 1);
        assert!(state.can_undo && !state.can_redo && !state.needs_save);
    }

    #[test]
    fn a_drag_a_save_finishes_keeps_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Param::Exposure, 0.3, false);
        edit.set(Param::Exposure, 0.4, false);
        // The app leaves before the slider says the drag ended.
        edit.save().unwrap();
        drop(edit);
        let reopened = open(&library, id, &path);
        assert_eq!(reopened.state().steps, ["Exposure +0.40"]);
        assert_eq!(exposure(&reopened), 0.4);
    }

    #[test]
    fn an_undo_is_saved_with_the_step_left_to_redo() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Param::Contrast, 0.2, true);
        edit.set(Param::Exposure, 1.0, true);
        assert!(edit.step(-1));
        assert_eq!(exposure(&edit), 0.);
        edit.save().unwrap();
        drop(edit);

        let mut reopened = open(&library, id, &path);
        let state = reopened.state();
        assert_eq!(state.steps.len(), 2);
        assert_eq!(state.applied, 1);
        assert!(state.can_redo);
        assert!(reopened.step(1));
        assert_eq!(exposure(&reopened), 1.0);
    }

    #[test]
    fn a_value_outside_the_slider_is_clamped_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Param::Exposure, 12., true);
        assert_eq!(exposure(&edit), edit.state().values[0].max);
    }

    #[test]
    fn a_failed_save_keeps_the_edit_unsaved_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Param::Exposure, 0.5, true);
        // The file is gone: its identity, which the edit is saved for, can't be read.
        std::fs::remove_file(&path).unwrap();
        assert!(edit.save().is_err());
        let state = edit.state();
        assert!(state.needs_save);
        assert!(state.save_error.is_some());
        // Nothing reached the catalog.
        assert!(!library.catalog().has_edit(PhotoId(id)).unwrap());
    }

    #[test]
    fn temp_moves_the_white_balance_as_the_desktop_does() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let before = edit.recipe().clone();
        let temp = edit.state().values[4].value;
        edit.set(Param::Temperature, temp + 500., true);
        assert_ne!(edit.recipe().wb, before.wb, "the multipliers follow Temp");
    }
}
