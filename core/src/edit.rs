//! Stage 5: the open photo's edit, with History, saved to the catalog.
//!
//! The edit is RAWmakase's own [`EditSession`], as the desktop's Develop keeps
//! it: every change goes through [`change_setting`], the model's one way to
//! change a develop setting (it recomputes the white balance when Temp or Tint
//! move), inside an editor frame, so a slider drag becomes one History step
//! named as the desktop names it. The edit and its History load from the
//! catalog the way the desktop resolves them, and save back in one transaction.

use crate::controls::{Control, Section, Switch};
use crate::library::Library;
use crate::render::RenderError;
use rawmakase_catalog::catalog::{HistoryUpdate, PhotoId};
use rawmakase_edit_session::history::Step;
use rawmakase_edit_session::{EditSession, Gesture};
use rawmakase_interop::export_settings::ExportOptions;
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::storage::Identity;
use std::path::PathBuf;
use std::sync::Arc;

/// The edit as the editor shows it.
#[derive(uniffi::Record)]
pub struct EditState {
    /// Every panel's sliders and switches, in the desktop's order.
    pub sections: Vec<Section>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// History, oldest first, as "Exposure +0.35"; `applied` of them are applied.
    pub steps: Vec<String>,
    pub applied: u32,
    /// The masks, in the recipe's order.
    pub masks: Vec<crate::masks::MaskInfo>,
    /// Heal and Clone spots.
    pub spots: u32,
    /// The crop, `[left, top, right, bottom]` as fractions of the photo as
    /// shown, straightened (docs/transform.md in RAWmakase).
    pub crop: Vec<f32>,
    /// Changes not saved to the catalog yet.
    pub needs_save: bool,
    /// Why the last save failed; the edit stays unsaved.
    pub save_error: Option<String>,
    /// What AI Denoise is doing for the photo on screen, when it is on.
    pub ai_denoise: Option<String>,
    /// The white balance menu: the current choice ("As Shot", "Auto", a
    /// preset or "Custom") and what it offers (none for a monochrome photo).
    pub white_balance: String,
    pub white_balance_choices: Vec<String>,
}

/// Rotate and Flip.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

/// A span narrower than the crop's 0.01 minimum, widened about its centre
/// and kept within 0..1: a little over 0.01, as exactly 0.01 can come out
/// a rounding error under it.
fn clamp_span(a: f32, b: f32) -> (f32, f32) {
    const HALF: f32 = 0.00505;
    let centre = ((a + b) / 2.).clamp(HALF, 1. - HALF);
    (centre - HALF, centre + HALF)
}

/// The open photo's edit: which photo, what it is, and its session.
pub(crate) struct OpenEdit {
    pub(crate) library: Arc<Library>,
    pub(crate) id: i64,
    pub(crate) path: PathBuf,
    pub(crate) metadata: Metadata,
    pub(crate) export: ExportOptions,
    /// The camera profiles the edit resolves with, for presets and Paste.
    pub(crate) profiles: Vec<Arc<rawmakase_model::camera_profiles::CameraProfile>>,
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
            profiles: profiles.to_vec(),
            session,
        })
    }

    pub(crate) fn recipe(&self) -> &Recipe {
        self.session.recipe()
    }

    /// Sets `control` to `value`, clamped to its slider's range, as one
    /// editor frame; `done` ends the drag, which becomes one History step.
    pub(crate) fn set(&mut self, control: Control, value: f32, done: bool) -> bool {
        let frame = self.session.begin();
        let named = control.set(self.session.recipe_mut(), &self.metadata, value);
        // Named on every frame, not only the last: a drag a save has to finish
        // (the app leaving mid-drag) keeps the name the slider gives it.
        if let Some((label, text)) = named {
            self.session.name_next_step(Step::new(label, text));
        }
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        let outcome = self.session.finish(frame, gesture);
        outcome == rawmakase_edit_session::FrameOutcome::Edited
    }

    /// Turns a panel switch or option on or off, as one History step.
    pub(crate) fn switch(&mut self, switch: Switch, on: bool) -> bool {
        self.session.finish_gesture();
        let mut after = self.session.recipe().clone();
        let Some((name, value)) = switch.set(&mut after, on) else {
            return false;
        };
        if after == *self.session.recipe() {
            return false;
        }
        self.session
            .change(Some(Step::new(name, value)), |r| *r = after);
        true
    }

    /// Sets the crop as one editor frame; `done` ends the drag, which becomes
    /// one History step ("Crop"). Kept within the photo and at least 0.01 of
    /// it each way, as `Recipe::validate` asks.
    pub(crate) fn set_crop(&mut self, crop: [f32; 4], done: bool) -> bool {
        let [mut l, mut t, mut r, mut b] = crop.map(|v| v.clamp(0., 1.));
        if r - l < 0.01 {
            (l, r) = clamp_span(l, r);
        }
        if b - t < 0.01 {
            (t, b) = clamp_span(t, b);
        }
        let frame = self.session.begin();
        self.session.recipe_mut().crop = [l, t, r, b];
        self.session.name_next_step(Step::new("Crop", ""));
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture) == rawmakase_edit_session::FrameOutcome::Edited
    }

    /// Edits point curve `channel` as one editor frame: adds a point at `at`
    /// (when `index` is `None`) or moves point `index` there. Returns the
    /// point's index, which the drag keeps moving; `done` ends the drag,
    /// one History step ("Point Curve" and the channel).
    pub(crate) fn curve_point(
        &mut self,
        channel: u32,
        index: Option<u32>,
        at: [f32; 2],
        done: bool,
    ) -> Option<u32> {
        crate::curve::curve(self.session.recipe(), channel)?;
        let frame = self.session.begin();
        let curve = crate::curve::curve_mut(self.session.recipe_mut(), channel)?;
        let index = match index {
            Some(i) => {
                curve.move_point(i as usize, at);
                i as usize
            }
            None => curve.insert(at),
        };
        let channel_name = crate::curve::CHANNELS[channel as usize];
        self.session
            .name_next_step(Step::new("Point Curve", channel_name));
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture);
        Some(index as u32)
    }

    /// Removes point `index` of curve `channel` (not an end point): one
    /// History step.
    pub(crate) fn remove_curve_point(&mut self, channel: u32, index: u32) -> bool {
        let mut after = self.session.recipe().clone();
        let removed = crate::curve::curve_mut(&mut after, channel)
            .is_some_and(|c| c.remove(index as usize));
        if removed {
            self.change("Point Curve", crate::curve::CHANNELS[channel as usize], |r| *r = after);
        }
        removed
    }

    /// Sets the RGB curve to one of Lightroom's built-in curves.
    pub(crate) fn set_builtin_curve(&mut self, name: &str) -> bool {
        let mut after = self.session.recipe().clone();
        if !crate::curve::set_builtin(&mut after, name) {
            return false;
        }
        self.change("Point Curve", name, |r| *r = after);
        true
    }

    /// Rotates or flips the photo as shown, keeping the crop on the same part
    /// of it, as the desktop's Rotate and Flip do: one History step.
    pub(crate) fn orient(&mut self, orientation: Orientation) -> bool {
        use rawmakase_engine::develop::{Mirror, QuarterTurn, mirror, turn};
        self.session.finish_gesture();
        let name = match orientation {
            Orientation::RotateLeft => "Rotate Left",
            Orientation::RotateRight => "Rotate Right",
            Orientation::FlipHorizontal => "Flip Horizontal",
            Orientation::FlipVertical => "Flip Vertical",
        };
        self.session
            .change(Some(Step::new(name, "")), |r| match orientation {
                Orientation::RotateLeft => turn(r, QuarterTurn::Left),
                Orientation::RotateRight => turn(r, QuarterTurn::Right),
                Orientation::FlipHorizontal => mirror(r, Mirror::Horizontal),
                Orientation::FlipVertical => mirror(r, Mirror::Vertical),
            });
        true
    }

    /// Applies `preset` as the desktop's Presets panel does, its Auto
    /// settings measured on `image`: one History step.
    pub(crate) fn apply_preset(
        &mut self,
        preset: &rawmakase_interop::xmp::Preset,
        image: &rawmakase_model::camera_data::CameraImage,
    ) -> anyhow::Result<Vec<String>> {
        self.session.finish_gesture();
        let applied = rawmakase_interop::presets::apply_to_photo(
            preset,
            self.session.recipe(),
            &self.metadata,
            &self.profiles,
            Some(&rawmakase_engine::develop::Measures(image)),
        )?;
        applied.recipe.validate()?;
        let name = rawmakase_interop::presets::display_name(&preset.name);
        self.session
            .change(Some(Step::new("Preset", name)), |r| *r = applied.recipe);
        Ok(applied.skipped)
    }

    /// The copied settings pasted over this photo's, as one History step.
    pub(crate) fn paste(&mut self) -> Option<Vec<String>> {
        self.session.finish_gesture();
        let (recipe, notes) = crate::transfer::paste(
            &self.library,
            self.session.recipe(),
            &self.metadata,
            &self.profiles,
        )?;
        recipe.validate().ok()?;
        self.session
            .change(Some(Step::new("Paste Settings", "")), |r| *r = recipe);
        Some(notes)
    }

    /// A change made outside a drag (a new mask, a stroke, a spot), as one
    /// History step named `name` `value`, kept only if the result is valid.
    pub(crate) fn change(&mut self, name: &str, value: &str, change: impl FnOnce(&mut Recipe)) {
        self.session.finish_gesture();
        let mut after = self.session.recipe().clone();
        change(&mut after);
        if after != *self.session.recipe() && after.validate().is_ok() {
            self.session
                .change(Some(Step::new(name, value)), |r| *r = after);
        }
    }

    /// Moves History one step back (`by` −1) or forward (+1).
    /// Sets white balance to As Shot or a named preset, as one History step.
    pub(crate) fn set_white_balance(&mut self, name: &str) -> bool {
        let mut after = self.session.recipe().clone();
        if !crate::wb::set_named(&mut after, &self.metadata, name) {
            return false;
        }
        self.change("White Balance", name, |r| *r = after);
        true
    }

    /// Shows History with `applied` of its steps applied, as tapping a step
    /// in the History list does (0 is the edit as it was opened).
    pub(crate) fn go_to(&mut self, applied: usize) -> bool {
        self.session.finish_gesture();
        self.session.jump(applied)
    }

    pub(crate) fn step(&mut self, by: i32) -> bool {
        self.session.finish_gesture();
        let (_, applied) = self.session.history().steps();
        let Some(target) = applied.checked_add_signed(by as isize) else {
            return false;
        };
        self.session.jump(target)
    }

    pub(crate) fn state(&self) -> EditState {
        let (steps, applied) = self.session.history().steps();
        EditState {
            sections: crate::controls::sections(self.session.recipe(), &self.metadata),
            can_undo: applied > 0,
            can_redo: applied < steps.len(),
            steps: steps
                .iter()
                .map(|s| format!("{} {}", s.name, s.value).trim().to_string())
                .collect(),
            applied: applied as u32,
            masks: crate::masks::infos(self.session.recipe()),
            spots: self.session.recipe().retouch.len() as u32,
            crop: self.session.recipe().crop.to_vec(),
            needs_save: self.session.save_state().needs_save(),
            save_error: self.session.save_state().message().map(str::to_string),
            ai_denoise: None,
            white_balance: crate::wb::current(self.session.recipe(), &self.metadata),
            white_balance_choices: crate::wb::choices(self.session.recipe(), &self.metadata),
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
    use crate::controls::{Control, Switch};
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

    fn slider(edit: &OpenEdit, label: &str) -> crate::controls::SliderState {
        edit.state()
            .sections
            .into_iter()
            .flat_map(|s| s.sliders)
            .find(|s| s.label == label)
            .unwrap()
    }

    fn exposure(edit: &OpenEdit) -> f32 {
        slider(edit, "Exposure").value
    }

    #[test]
    fn a_drag_is_one_named_step_and_survives_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        assert_eq!(edit.state().steps.len(), 0);

        edit.set(Control::Param { index: 2 }, 0.3, false);
        edit.set(Control::Param { index: 2 }, 0.4, false);
        edit.set(Control::Param { index: 2 }, 0.5, true);
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
        edit.set(Control::Param { index: 2 }, 0.3, false);
        edit.set(Control::Param { index: 2 }, 0.4, false);
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
        edit.set(Control::Param { index: 3 }, 0.2, true);
        edit.set(Control::Param { index: 2 }, 1.0, true);
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
        edit.set(Control::Param { index: 2 }, 12., true);
        assert_eq!(exposure(&edit), slider(&edit, "Exposure").max);
    }

    #[test]
    fn white_balance_presets_set_lightrooms_values_as_one_step() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        assert_eq!(edit.state().white_balance, "As Shot");
        assert!(edit.state().white_balance_choices.contains(&"Tungsten".to_string()));
        assert!(edit.set_white_balance("Daylight"));
        assert_eq!((edit.recipe().temperature, edit.recipe().tint), (5500., 10.));
        assert_eq!(edit.state().white_balance, "Daylight");
        assert_eq!(edit.state().steps, ["White Balance Daylight"]);
        assert!(edit.set_white_balance("As Shot"));
        assert_eq!(edit.state().white_balance, "As Shot");
        assert!(!edit.set_white_balance("Moonlight"));
    }

    #[test]
    fn a_curve_drag_adds_a_point_and_is_one_step() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let i = edit.curve_point(0, None, [0.5, 0.5], false).unwrap();
        edit.curve_point(0, Some(i), [0.5, 0.6], false);
        edit.curve_point(0, Some(i), [0.5, 0.65], true);
        assert_eq!(edit.recipe().curve.points.len(), 3);
        assert!((edit.recipe().curve.evaluate(0.5) - 0.65).abs() < 1e-3);
        assert_eq!(edit.state().steps, ["Point Curve RGB"]);
        // A point can't be dragged past its neighbours.
        edit.curve_point(2, None, [0.3, 0.4], true);
        let red = &edit.recipe().effects.channels[1];
        assert_eq!(red.points.len(), 3);
        edit.recipe().validate().unwrap();
        assert!(edit.remove_curve_point(0, i));
        assert!(!edit.remove_curve_point(0, 0), "an end point stays");
        assert!(edit.set_builtin_curve("Strong Contrast"));
        assert_eq!(crate::curve::state(edit.recipe(), 0).unwrap().name, "Strong Contrast");
    }

    #[test]
    fn tapping_a_history_step_shows_the_edit_as_it_was_then() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Control::Param { index: 2 }, 0.5, true);
        edit.set(Control::Param { index: 2 }, 1.0, true);
        edit.set(Control::Param { index: 3 }, 20., true);
        assert!(edit.go_to(1));
        assert_eq!((exposure(&edit), edit.state().applied), (0.5, 1));
        // The later steps stay, to go forward to.
        assert_eq!(edit.state().steps.len(), 3);
        assert!(edit.go_to(3));
        assert_eq!(exposure(&edit), 1.0);
        assert!(!edit.go_to(9), "past the last step");
    }

    #[test]
    fn a_switch_is_one_named_step() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        assert!(edit.switch(Switch::Monochrome, true));
        assert!(edit.recipe().effects.monochrome);
        assert_eq!(edit.state().steps, ["Treatment Black & White"]);
        // Turned on, the B&W Mix replaces the Color Mixer.
        let titles: Vec<_> = edit.state().sections.into_iter().map(|s| s.title).collect();
        assert!(titles.contains(&"B&W Mix".to_string()));
        assert!(!titles.iter().any(|t| t.starts_with("Color Mixer")));
        // Setting it as it is changes nothing.
        assert!(!edit.switch(Switch::Monochrome, true));
    }

    #[test]
    fn ai_denoise_switches_on_as_on_the_desktop() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let amount = edit
            .state()
            .sections
            .into_iter()
            .find(|s| s.title == "AI Denoise")
            .and_then(|s| s.sliders.into_iter().next())
            .unwrap();
        assert!(edit.switch(Switch::AiDenoise, true));
        assert!(edit.recipe().ai_denoise.renders());
        // As the desktop's switch: manual noise reduction goes to 0.
        assert_eq!(edit.recipe().noise_luma, 0.);
        assert_eq!(edit.recipe().noise_chroma, 0.);
        assert_eq!(edit.state().steps, ["AI Denoise On"]);
        // Amount is its own section's slider, at the desktop's default.
        assert_eq!((amount.label.as_str(), amount.value), ("Amount", 0.5));
    }

    #[test]
    fn a_crop_drag_is_one_step_and_stays_valid() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set_crop([0.1, 0.1, 0.9, 0.9], false);
        edit.set_crop([0.2, 0.15, 0.8, 0.85], true);
        assert_eq!(edit.state().steps, ["Crop"]);
        assert_eq!(edit.recipe().crop, [0.2, 0.15, 0.8, 0.85]);
        // Outside the photo, or too small: kept valid.
        edit.set_crop([-0.5, 0.5, 0.5, 0.5], true);
        edit.recipe().validate().unwrap();
    }

    #[test]
    fn rotating_keeps_the_crop_on_the_same_part_of_the_photo() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set_crop([0., 0., 0.5, 0.5], true);
        edit.orient(Orientation::RotateRight);
        assert_eq!(edit.recipe().rotation, 1);
        // The top-left quarter, turned clockwise, is the top-right one.
        assert_eq!(edit.recipe().crop, [0.5, 0., 1., 0.5]);
        assert_eq!(edit.state().steps.last().unwrap(), "Rotate Right");
    }

    #[test]
    fn a_failed_save_keeps_the_edit_unsaved_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Control::Param { index: 2 }, 0.5, true);
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
        let temp = slider(&edit, "Temp").value;
        edit.set(Control::Param { index: 0 }, temp + 500., true);
        assert_ne!(edit.recipe().wb, before.wb, "the multipliers follow Temp");
    }
}
