//! Stage 5: the open photo's edit, with History, saved to the catalog.
//!
//! The edit is RAWmakase's own [`EditSession`], as the desktop's Develop keeps
//! it: every change goes through [`change_setting`], the model's one way to
//! change a develop setting (it recomputes the white balance when Temp or Tint
//! move), inside an editor frame, so a slider drag becomes one History step
//! named as the desktop names it. The edit and its History load from the
//! catalog the way the desktop resolves them, and save back in one transaction.

use crate::controls::{Control, Section, Switch};
use crate::edit_session::history::Step;
use crate::edit_session::{EditSession, Gesture};
use crate::library::Library;
use crate::render::RenderError;
use rawmakase_catalog::catalog::{HistoryUpdate, PhotoId};
use rawmakase_interop::export_settings::ExportOptions;
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::transform::UprightMode;
use rawmakase_model::storage::Identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The edit as the editor shows it.
#[derive(uniffi::Record)]
pub struct EditState {
    /// The photo this is the edit of: while the next photo opens, the view
    /// still holds the last one's.
    pub photo: i64,
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
    /// How much of it is done (0–1) while it runs, for a progress bar.
    pub ai_denoise_progress: Option<f32>,
    /// The white balance menu: the current choice ("As Shot", "Auto", a
    /// preset or "Custom") and what it offers (none for a monochrome photo).
    pub white_balance: String,
    pub white_balance_choices: Vec<String>,
    /// The Amount of the preset just applied, until anything else changes
    /// the photo.
    pub preset_amount: Option<crate::transfer::PresetAmountState>,
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
    /// Where the Profile browser lists each profile.
    pub(crate) profile_groups: rawmakase_model::camera_profiles::Groups,
    pub(crate) session: EditSession,
    /// The Amount of the preset applied last, while it shows.
    pub(crate) amount: Option<crate::transfer::AmountShown>,
    /// The photo's starting settings (its raw defaults), which Reset and a
    /// slider's double tap go back to; resolved once when it opens.
    pub(crate) start: Recipe,
    /// The settings copied to Before ([`crate::compare`]); `None`, the
    /// photo's starting settings. Not saved, as in Lightroom.
    pub(crate) before: Option<Recipe>,
    /// The Color Grading wheel being dragged, and its color when the drag
    /// began, which History names the change from.
    pub(crate) wheel: Option<(u32, rawmakase_model::model::grading::HueSat)>,
    /// The Targeted Adjustment Tool's drag under way.
    pub(crate) targeted: Option<crate::targeted::TargetDrag>,
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
        let start = rawmakase_interop::raw_defaults::DevelopDefaults::default()
            .resolve(&metadata, profiles)
            .recipe;
        Ok(Self {
            start,
            library,
            id,
            path,
            export: resolved.export,
            profiles: profiles.to_vec(),
            profile_groups: rawmakase_model::camera_profiles::listed(&metadata).groups,
            metadata,
            session,
            amount: None,
            before: None,
            wheel: None,
            targeted: None,
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
        outcome == crate::edit_session::FrameOutcome::Edited
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

    /// Lightroom's Treatment, as the desktop sets it
    /// ([`TreatmentChoice::choose_treatment`]): converting to black & white
    /// the first time (a mix never set) applies the Auto mix measured on
    /// `image`, and choosing Color with a black & white profile goes back to
    /// the photo's own profile. One History step, named as the desktop names
    /// it; false when the Treatment was already that.
    ///
    /// [`TreatmentChoice::choose_treatment`]: rawmakase_engine::develop::black_white::TreatmentChoice::choose_treatment
    pub(crate) fn set_treatment(
        &mut self,
        black_white: bool,
        image: &rawmakase_model::camera_data::CameraImage,
    ) -> bool {
        use rawmakase_engine::develop::black_white::TreatmentChoice;
        use rawmakase_engine::develop::{AutoMix, ColorSpread};
        use rawmakase_model::model::recipe::Treatment;
        self.session.finish_gesture();
        let treatment = if black_white {
            Treatment::BlackWhite
        } else {
            Treatment::Color
        };
        if self.session.recipe().treatment() == treatment {
            return false;
        }
        let mut after = self.session.recipe().clone();
        // Measured only when the conversion uses it.
        let spread =
            (black_white && after.effects.gray_mix == [0.; 8]).then(|| ColorSpread::measure(image));
        let first = spread.as_ref().map(|spread| AutoMix {
            spread,
            metadata: &self.metadata,
        });
        after.choose_treatment(treatment, first, self.start.profile.clone(), &self.metadata);
        let name = if black_white {
            "Convert to Black & White"
        } else {
            "Convert to Color"
        };
        self.session
            .change(Some(Step::new(name, "")), |r| *r = after);
        true
    }

    /// Upright's mode, one History step; whether the photo must be analysed
    /// for it (no corrections for the mode yet).
    pub(crate) fn set_upright(&mut self, mode: UprightMode) -> bool {
        self.session.finish_gesture();
        if self.session.recipe().upright.mode == mode {
            return false;
        }
        let name = crate::geometry_upright::name(mode);
        self.session
            .change(Some(Step::new("Upright", name)), |r| r.upright.mode = mode);
        self.session.recipe().upright.needs_analysis()
    }

    /// The camera's reference exposure offset, when Calibration offers it as
    /// the desktop does: the photo has a camera profile and the edit's offset
    /// isn't it.
    pub(crate) fn camera_exposure_baseline(&self) -> Option<f32> {
        let baseline =
            rawmakase_model::camera_profiles::reference::baseline_exposure(&self.metadata);
        let r = self.session.recipe();
        (r.profile.is_some() && r.camera_exposure != baseline).then_some(baseline)
    }

    /// Applies [`Self::camera_exposure_baseline`], one History step, leaving
    /// the Exposure slider as it is.
    pub(crate) fn use_camera_exposure_baseline(&mut self) -> bool {
        let Some(baseline) = self.camera_exposure_baseline() else {
            return false;
        };
        self.change("Camera Exposure Baseline", "", |r| {
            r.camera_exposure = baseline
        });
        true
    }

    /// Guided Upright with `guides` (ends in Upright's frame), one History
    /// step named `step` as on the desktop (Add Guide, Move Guide, Delete
    /// Guide): the mode turns Guided and the guides are solved beside the
    /// photo's analysis, which must be there (the other modes'
    /// corrections). Returns why the guides correct less than they might, as
    /// the desktop's status line says it.
    pub(crate) fn set_guides(
        &mut self,
        guides: Vec<rawmakase_model::model::transform::UprightGuide>,
        step: &str,
    ) -> Option<String> {
        use rawmakase_engine::develop::guided::{self, Issue};
        self.session.finish_gesture();
        let metadata = self.metadata.clone();
        let count = guides.len();
        let mut issue = None;
        let value = match count {
            1 => "1 guide".to_string(),
            n => format!("{n} guides"),
        };
        self.session.change(Some(Step::new(step, value)), |r| {
            r.upright.mode = UprightMode::Guided;
            r.upright.guides = guides;
            issue = guided::store(r, &metadata);
        });
        issue
            .or((count < 2).then_some(Issue::TooFew))
            .map(|i| i.message().to_string())
    }

    /// Stores Upright's `corrections`, analysed on `analysed`, in every
    /// History state (and Before) they fit: the analysis is part of the
    /// photo, not an edit, so undoing the mode leaves nothing half-applied.
    /// States with corrections of their own keep them. Returns why the guides
    /// correct less than they might, for the photo as shown. Mirrors the
    /// desktop's `upright_ready`, which runs in its event loop.
    pub(crate) fn store_upright(
        &mut self,
        analysed: &Recipe,
        corrections: &[[f32; 9]],
    ) -> Option<String> {
        let inputs = crate::geometry_upright::inputs;
        let fits = |r: &Recipe| {
            inputs(r) == inputs(analysed) && r.upright.corrections == analysed.upright.corrections
        };
        let metadata = self.metadata.clone();
        let mut issue = None;
        for (i, r) in self
            .session
            .states_mut()
            .chain(self.before.as_mut())
            .enumerate()
        {
            if fits(r) {
                let solved =
                    rawmakase_engine::develop::upright::store(r, corrections, Some(&metadata));
                if i == 0 {
                    issue = solved.map(|s| s.message().to_string());
                }
            }
        }
        self.session.save_state_mut().mark_changed();
        issue
    }

    /// Uses `profile`, a look fitted to this photo's embedded JPEG, as its
    /// profile at 100%, as the desktop's Profile → Fit Camera Look does: Temp
    /// and Tint follow the new profile with the white balance unchanged, and
    /// the Treatment follows it, all one step.
    pub(crate) fn apply_fitted_look(
        &mut self,
        profile: Arc<rawmakase_model::camera_profiles::CameraProfile>,
    ) {
        use rawmakase_engine::develop::black_white::TreatmentChoice;
        self.session.finish_gesture();
        let metadata = self.metadata.clone();
        self.session
            .change(Some(Step::new("Fit Camera Look", "")), |r| {
                let old = r.profile.replace(profile);
                r.profile_amount = 1.;
                r.profile_changed(&metadata);
                r.follow_profile_treatment(old.as_deref(), None);
            });
    }

    /// The profiles this photo at `path` can use, as the editor lists them
    /// when it opens: the library's, with the camera's fitted default look
    /// where it has one. Slow when a look is fitted: call it without the
    /// edit's lock.
    pub(crate) fn list_profiles(
        path: &Path,
        metadata: &Metadata,
    ) -> (
        Vec<Arc<rawmakase_model::camera_profiles::CameraProfile>>,
        rawmakase_model::camera_profiles::Groups,
    ) {
        let listed = rawmakase_model::camera_profiles::listed(metadata);
        let profiles = rawmakase_decode::camera_look::with_default(
            path,
            metadata,
            listed.profiles,
            rawmakase_cancel::Cancel::NEVER,
        );
        (profiles, listed.groups)
    }

    /// Moves Color Grading wheel `region` (shadows, midtones, highlights,
    /// global) to the color at `offset`, in wheel radii from its centre with
    /// y down, or with `None` back to no color, as one editor frame; `done` ends the drag, one History step
    /// named as the desktop's wheel names it ("Shadow Hue & Saturation").
    pub(crate) fn set_grade_wheel(
        &mut self,
        region: u32,
        offset: Option<[f32; 2]>,
        done: bool,
    ) -> bool {
        use rawmakase_model::model::grading::{HueSat, wheel_step};
        let Some((_, word)) = crate::controls::REGIONS.get(region as usize) else {
            return false;
        };
        fn grade(r: &mut Recipe, region: u32) -> &mut [f32; 3] {
            if region == 3 {
                &mut r.effects.global_grade
            } else {
                &mut r.grading[region as usize]
            }
        }
        let before = HueSat::of(grade(&mut self.session.recipe().clone(), region));
        let start = match self.wheel {
            Some((r, start)) if r == region => start,
            _ => before,
        };
        self.wheel = (!done).then_some((region, start));
        // A double tap's reset, as the desktop's double click: no color and
        // hue back to 0, not only the saturation.
        let color = match offset {
            None => HueSat::NEUTRAL,
            Some(offset) => HueSat::at(offset, before.hue),
        };
        if color == before && done && start == before {
            self.wheel = None;
            return false;
        }
        let frame = self.session.begin();
        color.store(grade(self.session.recipe_mut(), region));
        let (name, value) = wheel_step(word, start, color);
        self.session.name_next_step(Step::new(name, value));
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture) == crate::edit_session::FrameOutcome::Edited
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
        self.session.finish(frame, gesture) == crate::edit_session::FrameOutcome::Edited
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

    /// A change made by dragging (a gradient's handle, say), as one editor
    /// frame; `done` ends the drag, one History step named `name`.
    pub(crate) fn drag(
        &mut self,
        name: &str,
        done: bool,
        change: impl FnOnce(&mut Recipe) -> bool,
    ) -> bool {
        let frame = self.session.begin();
        let changed = change(self.session.recipe_mut());
        self.session.name_next_step(Step::new(name, ""));
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture);
        changed
    }

    /// Removes point `index` of curve `channel` (not an end point): one
    /// History step.
    pub(crate) fn remove_curve_point(&mut self, channel: u32, index: u32) -> bool {
        let mut after = self.session.recipe().clone();
        let removed =
            crate::curve::curve_mut(&mut after, channel).is_some_and(|c| c.remove(index as usize));
        if removed {
            self.change(
                "Point Curve",
                crate::curve::CHANNELS[channel as usize],
                |r| *r = after,
            );
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
        let before = self.session.recipe().clone();
        self.session
            .change(Some(Step::new("Preset", name)), |r| *r = applied.recipe);
        // Kept once: every Amount is computed from these again.
        self.amount =
            crate::transfer::AmountShown::new(preset, before, self.session.recipe().clone());
        Ok(applied.skipped)
    }

    /// Sets the Amount of the preset just applied (0–2, 1 = 100%) as one
    /// editor frame; `done` ends the drag, one History step ("Preset
    /// Amount" and the percentage). Does nothing once anything else has
    /// changed the photo, which ends the Amount.
    pub(crate) fn set_preset_amount(&mut self, amount: f32, done: bool) -> bool {
        let current = self.session.recipe();
        let Some(shown) = self.amount.as_mut().filter(|a| a.shown == *current) else {
            self.amount = None;
            return false;
        };
        let amount = amount.clamp(
            *rawmakase_interop::presets::amount::RANGE.start(),
            *rawmakase_interop::presets::amount::RANGE.end(),
        );
        let target = shown.scale.at(amount, &self.metadata);
        shown.amount = amount;
        shown.shown = target.clone();
        let frame = self.session.begin();
        // Named only when it changes the photo: a name left over would name
        // the next edit.
        if target != *self.session.recipe() {
            *self.session.recipe_mut() = target;
            self.session
                .name_next_step(Step::new("Preset Amount", format!("{:.0}", amount * 100.)));
        }
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture) == crate::edit_session::FrameOutcome::Edited
    }

    /// Saves this photo's settings in `groups` as a new preset, as the
    /// desktop's New Develop Preset does; returns its id.
    pub(crate) fn create_preset(
        &self,
        user: &rawmakase_interop::presets::user::UserPresets,
        name: &str,
        group: &str,
        groups: &[u32],
    ) -> anyhow::Result<String> {
        let path = crate::transfer::create_preset(
            user,
            self.session.recipe(),
            &self.metadata,
            name,
            group,
            groups,
        )?;
        Ok(path.display().to_string())
    }

    /// Lightroom's Update with Current Settings: `preset` keeps its name,
    /// group and the setting groups it had, with this photo's values.
    pub(crate) fn update_preset(
        &self,
        user: &rawmakase_interop::presets::user::UserPresets,
        preset: &rawmakase_interop::xmp::Preset,
    ) -> anyhow::Result<()> {
        crate::transfer::update_preset(user, preset, self.session.recipe(), &self.metadata)
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

    /// The settings of the photo left last pasted over this one's, as the
    /// desktop's Paste from Previous: one History step. `None` with no
    /// previous photo, or nothing to change.
    pub(crate) fn paste_previous(&mut self) -> Option<Vec<String>> {
        self.session.finish_gesture();
        let (recipe, notes) = crate::transfer::paste_previous(
            &self.library,
            self.session.recipe(),
            &self.metadata,
            &self.profiles,
        )?;
        recipe.validate().ok()?;
        if recipe == *self.session.recipe() {
            return None;
        }
        self.session
            .change(Some(Step::new("Paste from Previous", "")), |r| *r = recipe);
        Some(notes)
    }

    /// Keeps this photo's settings for the next photo's Paste from Previous.
    pub(crate) fn leave(&self) {
        crate::transfer::remember_previous(&self.library, self.session.recipe(), &self.metadata);
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

    /// Starts a Targeted Adjustment drag over `sample`, the pixel where it
    /// began, taken of the edit `sampled`. False when the edit has changed
    /// since, or the colour is one the tool leaves alone.
    pub(crate) fn start_targeted(
        &mut self,
        kind: crate::targeted::TargetKind,
        sample: &rawmakase_engine::develop::targeted::TargetSample,
        sampled: &Recipe,
    ) -> bool {
        self.session.finish_gesture();
        self.targeted = (self.session.recipe() == sampled)
            .then(|| crate::targeted::TargetDrag::new(kind, sample, sampled))
            .flatten();
        self.targeted.is_some()
    }

    /// The drag has gone `up` points up from where it started (down is
    /// negative): the sliders follow, as one editor frame; `done` ends it, one
    /// History step named as the desktop names it ("Orange Saturation").
    /// False once the drag has ended, or something else changed the edit.
    pub(crate) fn drag_targeted(&mut self, up: f32, done: bool) -> bool {
        let Some(drag) = self.targeted.take() else {
            return false;
        };
        if *self.session.recipe() != drag.last {
            return false;
        }
        let frame = self.session.begin();
        drag.weights.apply(
            &drag.start,
            up * rawmakase_engine::develop::targeted::DRAG_RATE,
            self.session.recipe_mut(),
        );
        let (name, value) = drag.weights.step(self.session.recipe());
        self.session.name_next_step(Step::new(name, value));
        let gesture = if done {
            Gesture::Released
        } else {
            Gesture::Held
        };
        let edited =
            self.session.finish(frame, gesture) == crate::edit_session::FrameOutcome::Edited;
        if !done {
            self.targeted = Some(crate::targeted::TargetDrag {
                last: self.session.recipe().clone(),
                ..drag
            });
        }
        edited
    }

    /// Sets Straighten so the line drawn from `from` to `to` (view points,
    /// y down, over the photo as shown) is level or, nearer upright, plumb,
    /// as the desktop's ruler does: through the Straighten slider, one
    /// History step. False for a line too short to go by.
    pub(crate) fn straighten_with_ruler(&mut self, from: [f32; 2], to: [f32; 2]) -> bool {
        use rawmakase_model::model::params::ParameterId;
        let Some(index) = crate::controls::index_of(ParameterId::Straighten) else {
            return false;
        };
        let current = self.session.recipe().straighten;
        match rawmakase_model::model::transform::ruler_angle(from, to, current) {
            Some(angle) => self.set(Control::Param { index }, angle, true),
            None => false,
        }
    }

    /// Moves the parametric curve's split `i` (0 to 2) to `t`, kept apart
    /// from its neighbours, or with `None` back to where it starts, as one
    /// editor frame; `done` ends the drag, one History step. The panel turns
    /// on, as any change to it does.
    pub(crate) fn set_parametric_split(&mut self, i: usize, t: Option<f32>, done: bool) -> bool {
        use rawmakase_model::model::effects::Effects;
        use rawmakase_model::model::panels::{Panel, PanelState};
        if i > 2 {
            return false;
        }
        let frame = self.session.begin();
        let r = self.session.recipe_mut();
        match t {
            Some(t) => r.effects.move_split(i, t),
            None => r.effects.splits[i] = Effects::DEFAULT_SPLITS[i],
        }
        r.panels.set(Panel::ToneCurve, PanelState::On);
        self.session
            .name_next_step(Step::new("Tone Curve", "Splits"));
        let gesture = if done || t.is_none() {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture) == crate::edit_session::FrameOutcome::Edited
    }

    /// Gives the photo the Point Curve menu's curve `index` (built-in ones
    /// first, then saved ones): one History step ("Point Curve", its name),
    /// none when it already has it.
    pub(crate) fn choose_point_curve(&mut self, index: usize) -> bool {
        use rawmakase_model::model::panels::{Panel, PanelState};
        let mut after = self.session.recipe().clone();
        let Some(name) = crate::tone::choose(&mut after, index) else {
            return false;
        };
        after.panels.set(Panel::ToneCurve, PanelState::On);
        if after == *self.session.recipe() {
            return false;
        }
        self.change("Point Curve", &name, |r| *r = after);
        true
    }

    /// Duplicates mask `index` as the next mask: one History step. Returns
    /// the copy's index; `None` when there is no such mask or no room.
    pub(crate) fn duplicate_mask(&mut self, index: usize) -> Option<usize> {
        let mut after = self.session.recipe().clone();
        let copy = rawmakase_model::model::masks::duplicate(&mut after.masks, index)?;
        self.change("Duplicate Mask", "", |r| *r = after);
        Some(copy)
    }

    /// Mask `index`'s sliders back to a new mask's, its shape and Amount
    /// kept: one History step; false when they already are.
    pub(crate) fn reset_mask_sliders(&mut self, index: usize) -> bool {
        let fresh = rawmakase_model::model::masks::LocalAdjust::default();
        if self
            .session
            .recipe()
            .masks
            .get(index)
            .is_none_or(|m| m.adjust == fresh)
        {
            return false;
        }
        self.change("Reset Mask Sliders", "", |r| r.masks[index].adjust = fresh);
        true
    }

    /// Adds a Point Color swatch of `source` as one History step ("Point
    /// Color", "Add Swatch", as the desktop names it). Returns its index, or
    /// why it was refused.
    pub(crate) fn add_point_color(&mut self, source: [f32; 3]) -> Result<usize, String> {
        self.session.finish_gesture();
        let mut after = self.session.recipe().clone();
        let index = crate::point_color::add(&mut after, source)?;
        self.session
            .change(Some(Step::new("Point Color", "Add Swatch")), |r| *r = after);
        Ok(index)
    }

    /// Moves point `point` of swatch `swatch`'s range `which` (0 hue, 1
    /// saturation, 2 luminance) to `t`, kept where Camera Raw accepts the
    /// swatch, as one editor frame; `done` ends the drag, one History step
    /// named as the desktop names it ("Point Color Hue Range", the points).
    /// `None` for `point` resets the range, one step.
    pub(crate) fn set_point_color_range(
        &mut self,
        swatch: usize,
        which: usize,
        point: Option<usize>,
        t: f32,
        done: bool,
    ) -> bool {
        let Some(&which) = crate::point_color::RANGES.get(which) else {
            return false;
        };
        if swatch >= self.session.recipe().point_colors.len() {
            return false;
        }
        let frame = self.session.begin();
        let p = &mut self.session.recipe_mut().point_colors[swatch];
        match point {
            Some(k) => p.move_range_point(which, k, t),
            None => p.reset_range(which),
        }
        let shown: Vec<String> = p
            .range_points(which)
            .iter()
            .map(|v| format!("{:.0}", v * 100.))
            .collect();
        self.session.name_next_step(Step::new(
            format!("Point Color {}", which.label()),
            shown.join(" / "),
        ));
        let gesture = if done || point.is_none() {
            Gesture::Released
        } else {
            Gesture::Held
        };
        self.session.finish(frame, gesture) == crate::edit_session::FrameOutcome::Edited
    }

    /// Deletes swatch `index`, or all of them: one History step.
    pub(crate) fn delete_point_color(&mut self, index: Option<usize>) -> bool {
        let list = &self.session.recipe().point_colors;
        let value = match index {
            Some(i) if i < list.len() => "Delete Swatch",
            None if !list.is_empty() => "Delete All Swatches",
            _ => return false,
        };
        self.change("Point Color", value, |r| match index {
            Some(i) => {
                r.point_colors.remove(i);
            }
            None => r.point_colors.clear(),
        });
        true
    }

    /// The photo's starting settings: its raw defaults, as a photo opened
    /// with no edit gets them.
    pub(crate) fn starting_settings(&self) -> Recipe {
        self.start.clone()
    }

    /// Back to the photo's starting settings, as the desktop's Reset
    /// Settings: one History step.
    pub(crate) fn reset(&mut self) -> bool {
        self.session.finish_gesture();
        let defaults = self.starting_settings();
        if defaults == *self.session.recipe() {
            return false;
        }
        let was = self.session.replace(defaults);
        self.session
            .commit(was, Some(Step::new("Reset Settings", "")));
        true
    }

    /// Lightroom's Clear History: the edit stays as it is, with no steps to
    /// go back through; saved as such.
    pub(crate) fn clear_history(&mut self) {
        self.session.finish_gesture();
        let recipe = self.session.recipe().clone();
        let mut session = EditSession::default();
        session.replace(recipe);
        session.save_state_mut().mark_changed();
        self.session = session;
        self.amount = None;
    }

    /// Moves History one step back (`by` −1) or forward (+1).
    /// The profiles this photo can use, as the Profile browser lists them.
    pub(crate) fn profiles(&self) -> Vec<crate::profile::ProfileInfo> {
        crate::profile::list(
            self.session.recipe(),
            &self.metadata,
            &self.profiles,
            &self.profile_groups,
        )
    }

    /// Chooses profile `index` (`None`: the camera matrix), as one History
    /// step ("Profile" and its title).
    pub(crate) fn set_profile(&mut self, index: Option<u32>) -> bool {
        let mut after = self.session.recipe().clone();
        let Some(title) = crate::profile::choose(
            &mut after,
            &self.metadata,
            &self.profiles,
            &self.profile_groups,
            index,
        ) else {
            return false;
        };
        self.change("Profile", &title, |r| *r = after);
        true
    }

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
            photo: self.id,
            sections: crate::controls::sections(self.session.recipe(), &self.metadata, &self.start),
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
            ai_denoise_progress: None,
            white_balance: crate::wb::current(self.session.recipe(), &self.metadata),
            white_balance_choices: crate::wb::choices(self.session.recipe(), &self.metadata),
            preset_amount: self
                .amount
                .as_ref()
                .and_then(|a| a.state(self.session.recipe())),
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
pub(crate) mod tests {
    use super::*;
    use crate::controls::{Control, Switch};
    use rawmakase_cancel::Cancel;
    use std::path::Path;

    /// A catalog with one of RAWmakase's synthetic chart DNGs, and the photo's id.
    pub(crate) fn library(dir: &Path) -> (Arc<Library>, i64, PathBuf) {
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

    pub(crate) fn open(library: &Arc<Library>, id: i64, path: &Path) -> OpenEdit {
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

    pub(crate) fn exposure(edit: &OpenEdit) -> f32 {
        slider(edit, "Exposure").value
    }

    #[test]
    fn reset_is_one_step_and_clear_history_keeps_the_edit_with_no_steps() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let fresh = edit.recipe().clone();
        edit.set(Control::Param { index: 2 }, 1.2, true);
        edit.set(Control::Param { index: 3 }, 0.4, true);
        assert!(edit.reset());
        assert_eq!(*edit.recipe(), fresh);
        assert_eq!(edit.state().steps.last().unwrap(), "Reset Settings");
        assert!(!edit.reset(), "already the starting settings");

        edit.set(Control::Param { index: 2 }, 0.75, true);
        edit.clear_history();
        let state = edit.state();
        assert_eq!(
            (state.steps.len(), state.applied, state.can_undo),
            (0, 0, false)
        );
        assert_eq!(exposure(&edit), 0.75);
        assert!(state.needs_save);
        edit.save().unwrap();
        drop(edit);
        let reopened = open(&library, id, &path);
        assert_eq!(exposure(&reopened), 0.75);
        assert!(
            reopened.state().steps.is_empty(),
            "the cleared History is what was saved"
        );
    }

    #[test]
    fn a_virtual_copy_has_its_own_edit_of_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut master = open(&library, id, &path);
        master.set(Control::Param { index: 2 }, 0.5, true);
        master.save().unwrap();
        drop(master);

        let copy = library.create_virtual_copy(id).unwrap();
        let photos = library.photos().unwrap();
        let listed = photos.iter().find(|p| p.id == copy).unwrap();
        assert!(listed.is_copy);
        assert_eq!(listed.copy_name, "Copy 1");
        // The same file, as the file system resolves it (/private/var for /var).
        assert_eq!(
            Path::new(&listed.path).canonicalize().unwrap(),
            path.canonicalize().unwrap()
        );
        let mut edit = open(&library, copy, &path);
        assert_eq!(exposure(&edit), 0.5, "starts with the master's edit");
        edit.set(Control::Param { index: 2 }, -1., true);
        edit.save().unwrap();
        drop(edit);
        assert_eq!(
            exposure(&open(&library, id, &path)),
            0.5,
            "the master's is its own"
        );

        library.rename_virtual_copy(copy, "Dark".into()).unwrap();
        library.set_copy_as_master(copy).unwrap();
        let photos = library.photos().unwrap();
        assert!(!photos.iter().find(|p| p.id == copy).unwrap().is_copy);
        assert!(photos.iter().find(|p| p.id == id).unwrap().is_copy);
        assert!(
            library.remove_virtual_copy(copy).is_err(),
            "a master isn't removed"
        );
        library.remove_virtual_copy(id).unwrap();
        assert_eq!(library.photos().unwrap().len(), 1);
        assert!(path.exists(), "the file stays");
        assert_eq!(exposure(&open(&library, copy, &path)), -1.);
    }

    #[test]
    fn a_slider_resets_to_the_photos_starting_value_temp_to_as_shot() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let as_shot = slider(&edit, "Temp").value;
        edit.set(Control::Param { index: 2 }, 1.3, true);
        let temp = slider(&edit, "Temp");
        edit.set(temp.control, temp.value + 1500., true);
        assert_ne!(slider(&edit, "Temp").value, as_shot);
        assert_eq!(slider(&edit, "Temp").reset, as_shot, "as shot");
        assert_eq!(slider(&edit, "Exposure").reset, 0.);
        let temp = slider(&edit, "Temp");
        edit.set(temp.control, temp.reset, true);
        assert!(
            (slider(&edit, "Temp").value - as_shot).abs() < 1.,
            "back as shot"
        );
    }

    #[test]
    fn the_edit_state_names_its_photo() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        assert_eq!(open(&library, id, &path).state().photo, id);
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
        assert!(
            edit.state()
                .white_balance_choices
                .contains(&"Tungsten".to_string())
        );
        assert!(edit.set_white_balance("Daylight"));
        assert_eq!(
            (edit.recipe().temperature, edit.recipe().tint),
            (5500., 10.)
        );
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
        assert_eq!(
            crate::curve::state(edit.recipe(), 0).unwrap().name,
            "Strong Contrast"
        );
    }

    #[test]
    fn choosing_a_profile_is_one_step_and_starts_at_full_amount() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let listed = edit.profiles();
        assert_eq!(listed.iter().filter(|p| p.selected).count(), 1);
        let other = listed
            .iter()
            .find(|p| !p.selected && p.index.is_some())
            .expect("a second profile for the chart");
        let title = other.title.clone();
        assert!(edit.set_profile(other.index));
        assert_eq!(edit.recipe().profile_amount, 1.);
        assert_eq!(edit.state().steps, [format!("Profile {title}")]);
        assert!(
            edit.profiles()
                .iter()
                .any(|p| p.selected && p.title == title)
        );
        assert!(
            !edit.set_profile(other.index),
            "choosing it again changes nothing"
        );
        edit.recipe().validate().unwrap();
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

    /// Calibration's "Use camera exposure baseline" sets the camera's offset
    /// in one step and is offered only while the edit's differs.
    #[test]
    fn the_camera_exposure_baseline_is_offered_until_used() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let baseline =
            rawmakase_model::camera_profiles::reference::baseline_exposure(&edit.metadata);
        let exposure = edit.recipe().exposure;
        edit.change("Test", "", |r| r.camera_exposure = baseline + 0.5);
        assert!(
            edit.recipe().profile.is_some(),
            "the chart has a camera profile"
        );
        assert_eq!(edit.camera_exposure_baseline(), Some(baseline));
        assert!(edit.use_camera_exposure_baseline());
        assert_eq!(edit.recipe().camera_exposure, baseline);
        assert_eq!(edit.recipe().exposure, exposure, "Exposure as it was");
        assert_eq!(edit.camera_exposure_baseline(), None);
        assert!(!edit.use_camera_exposure_baseline());
    }

    /// The HDR switch sits under Light, as Lightroom's, and is one step that
    /// the desktop reads as Lightroom's `crs:HDREditMode`.
    #[test]
    fn the_hdr_switch_is_one_step_under_light() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let light = crate::controls::sections(edit.recipe(), &edit.metadata, edit.recipe())
            .into_iter()
            .find(|s| s.title == "Light")
            .unwrap();
        assert_eq!(light.switches[0].label, "High Dynamic Range");
        assert!(!light.switches[0].on);
        assert!(edit.switch(Switch::Hdr, true));
        assert!(edit.recipe().hdr.is_active());
        assert_eq!(edit.state().steps, ["HDR On"]);
        assert!(!edit.switch(Switch::Hdr, true), "already on");
        // A raw photo's HDR edit renders, as on the desktop.
        rawmakase_model::model::hdr::check_for(edit.recipe(), &edit.metadata).unwrap();
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

    /// Choosing an Upright mode is one step and asks for an analysis; the
    /// analysis reaches every History state it fits, so undoing the choice
    /// leaves the photo's corrections in place, as on the desktop.
    #[test]
    fn upright_is_analysed_once_and_kept_through_history() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let image = rawmakase_decode::photo::open(&path)
            .unwrap()
            .develop(rawmakase_model::camera_data::Decode::Half, Cancel::NEVER)
            .unwrap();
        assert!(edit.set_upright(UprightMode::Auto), "needs an analysis");
        let analysed = edit.recipe().clone();
        let corrections = rawmakase_engine::develop::upright::analyse(&image, &analysed);
        assert!(!corrections.is_empty());
        edit.store_upright(&analysed, &corrections);
        assert!(!edit.recipe().upright.needs_analysis());
        assert_eq!(edit.state().steps, ["Upright Auto"]);
        // Every state has the analysis: the one before the choice too.
        assert!(
            edit.session
                .states_mut()
                .all(|r| r.upright.corrections == corrections)
        );
        // Another mode uses the same analysis: no new one is needed.
        assert!(!edit.set_upright(UprightMode::Level));
        assert!(!edit.set_upright(UprightMode::Level), "already Level");
    }

    /// Guides are one History step that turns the mode Guided; one guide is
    /// too few, as the desktop says, and two solve a correction.
    #[test]
    fn guides_are_one_step_and_two_solve_a_correction() {
        use rawmakase_model::model::transform::UprightGuide;
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let image = rawmakase_decode::photo::open(&path)
            .unwrap()
            .develop(rawmakase_model::camera_data::Decode::Half, Cancel::NEVER)
            .unwrap();
        let analysed = edit.recipe().clone();
        let corrections = rawmakase_engine::develop::upright::analyse(&image, &analysed);
        edit.store_upright(&analysed, &corrections);
        let tilted = |x: f32| UprightGuide {
            a: [x, -0.4],
            b: [x + 0.03, 0.4],
        };
        let one = edit.set_guides(vec![tilted(-0.3)], "Add Guide");
        assert!(one.is_some(), "one guide is too few");
        assert_eq!(edit.recipe().upright.mode, UprightMode::Guided);
        let two = edit.set_guides(vec![tilted(-0.3), tilted(0.3)], "Add Guide");
        assert_eq!(two, None);
        assert_eq!(edit.recipe().upright.guides.len(), 2);
        assert_eq!(
            edit.state().steps,
            ["Add Guide 1 guide", "Add Guide 2 guides"]
        );
    }

    /// A look fitted to the photo's embedded JPEG becomes its profile at
    /// 100%, one step named as the desktop names it. (The fit itself is
    /// RAWmakase's, tested there: no test RAW here has an embedded JPEG, so a
    /// listed profile stands in for the fitted one.)
    #[test]
    fn a_fitted_camera_look_becomes_the_profile_at_full_amount() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let look = edit
            .profiles
            .iter()
            .find(|p| Some(*p) != edit.recipe().profile.as_ref())
            .cloned()
            .expect("another profile");
        edit.session
            .change(Some(Step::new("Profile Amount", "")), |r| {
                r.profile_amount = 0.4
            });
        edit.apply_fitted_look(look.clone());
        assert_eq!(edit.recipe().profile.as_ref(), Some(&look));
        assert_eq!(edit.recipe().profile_amount, 1.);
        assert_eq!(
            edit.state().steps.last().map(String::as_str),
            Some("Fit Camera Look")
        );
    }

    /// Converting to black & white sets the photo's Auto mix the first time,
    /// as the desktop (and Lightroom) do, and later conversions keep the mix;
    /// each is one step named as the desktop names it.
    #[test]
    fn converting_to_black_and_white_applies_the_auto_mix_once() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let image = rawmakase_decode::photo::open(&path)
            .unwrap()
            .develop(rawmakase_model::camera_data::Decode::Half, Cancel::NEVER)
            .unwrap();
        assert!(edit.set_treatment(true, &image));
        let mix = edit.recipe().effects.gray_mix;
        assert!(edit.recipe().effects.monochrome);
        assert_ne!(mix, [0.; 8], "the Auto mix");
        assert!(!edit.set_treatment(true, &image), "already black & white");
        assert!(edit.set_treatment(false, &image));
        assert!(edit.set_treatment(true, &image));
        assert_eq!(edit.recipe().effects.gray_mix, mix, "the mix is kept");
        assert_eq!(
            edit.state().steps,
            [
                "Convert to Black & White",
                "Convert to Color",
                "Convert to Black & White"
            ]
        );
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
    fn point_color_swatches_are_added_changed_and_deleted_as_steps() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let steps = edit.state().steps.len();
        assert_eq!(edit.add_point_color([0.5, 0.6, 0.4]), Ok(0));
        assert!(
            edit.add_point_color([0.5, 0.6, 0.4]).is_err(),
            "the same color"
        );
        assert_eq!(edit.add_point_color([3.5, 0.5, 0.3]), Ok(1));
        edit.set(
            Control::PointColor {
                swatch: 1,
                which: 1,
            },
            -0.5,
            true,
        );
        assert_eq!(edit.recipe().point_colors[1].shift[1], -0.5);
        assert!(edit.delete_point_color(Some(0)));
        assert_eq!(edit.recipe().point_colors.len(), 1);
        let names = edit.state().steps;
        assert_eq!(names.len(), steps + 4, "{names:?}");
        assert!(names[steps].starts_with("Point Color"), "{names:?}");

        assert!(edit.delete_point_color(None));
        assert!(edit.recipe().point_colors.is_empty());
        assert!(!edit.delete_point_color(None), "nothing left");
        edit.step(-1);
        assert_eq!(edit.recipe().point_colors.len(), 1);
    }

    #[test]
    fn a_range_handle_drag_is_one_step_and_keeps_the_swatch_valid() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.add_point_color([0.5, 0.6, 0.4]).unwrap();
        let fresh = edit.recipe().point_colors[0];
        let steps = edit.state().steps.len();
        // The inner saturation point dragged past the sampled value stops at it.
        for (t, done) in [(0.7, false), (0.9, false), (0.95, true)] {
            edit.set_point_color_range(0, 1, Some(1), t, done);
        }
        let p = edit.recipe().point_colors[0];
        assert_eq!(p.saturation_range[1], 0.6);
        assert!(p.is_valid());
        let names = edit.state().steps;
        assert_eq!(names.len(), steps + 1, "{names:?}");
        assert!(
            names.last().unwrap().starts_with("Point Color Sat Range"),
            "{names:?}"
        );

        assert!(edit.set_point_color_range(0, 1, None, 0., true), "reset");
        assert_eq!(
            edit.recipe().point_colors[0].saturation_range,
            fresh.saturation_range
        );
        assert!(
            !edit.set_point_color_range(3, 1, Some(0), 0.5, true),
            "no such swatch"
        );
    }

    #[test]
    fn a_mask_is_duplicated_after_itself_and_its_sliders_reset_as_steps() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let mut r = edit.recipe().clone();
        r.masks.push(rawmakase_model::model::masks::MaskGroup {
            name: "Sky".into(),
            ..Default::default()
        });
        r.masks[0].adjust.exposure = 0.5;
        r.masks[0].amount = 0.7;
        edit.change("Test", "", |x| *x = r);
        let steps = edit.state().steps.len();

        assert_eq!(edit.duplicate_mask(0), Some(1));
        let masks = &edit.recipe().masks;
        assert_eq!((masks.len(), masks[1].name.as_str()), (2, "Sky copy"));
        assert_eq!(masks[1].adjust.exposure, 0.5, "the same adjustment");

        assert!(edit.reset_mask_sliders(1));
        let copy = &edit.recipe().masks[1];
        assert_eq!(
            (copy.adjust.exposure, copy.amount),
            (0., 0.7),
            "Amount kept"
        );
        assert_eq!(
            edit.recipe().masks[0].adjust.exposure,
            0.5,
            "the original's kept"
        );
        assert!(!edit.reset_mask_sliders(1), "already reset");
        assert_eq!(edit.state().steps.len(), steps + 2);
        assert_eq!(edit.duplicate_mask(7), None);
    }

    #[test]
    fn the_ruler_levels_a_line_through_the_straighten_slider() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let steps = edit.state().steps.len();
        // A line falling 5° to the right is levelled by turning it back.
        let fall = 300. * 5f32.to_radians().tan();
        assert!(edit.straighten_with_ruler([0., 0.], [300., fall]));
        assert!(
            (edit.recipe().straighten + 5.).abs() < 0.01,
            "{}",
            edit.recipe().straighten
        );
        let names = edit.state().steps;
        assert_eq!(names.len(), steps + 1);
        assert!(names.last().unwrap().starts_with("Angle"), "{names:?}");
        assert!(!edit.straighten_with_ruler([0., 0.], [3., 1.]), "too short");
    }

    #[test]
    fn a_targeted_drag_on_orange_raises_its_saturation_as_one_step() {
        use crate::targeted::TargetKind;
        use rawmakase_engine::develop::targeted::TargetSample;
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let steps = edit.state().steps.len();
        // A skin tone, as the measured mixer and the B&W mix see it.
        let skin = TargetSample {
            tone: 0.5,
            mixer: [0.42, 0.30, 0.22],
            color: [0.7, 0.05, 0.06],
        };
        let sampled = edit.recipe().clone();
        assert!(edit.start_targeted(TargetKind::Saturation, &skin, &sampled));
        for (up, done) in [(40., false), (90., false), (125., true)] {
            assert!(edit.drag_targeted(up, done));
        }
        let orange = edit.recipe().hsl[1][1];
        assert!(
            orange > 0.4,
            "Orange Saturation moved with the drag: {orange}"
        );
        let names = edit.state().steps;
        assert_eq!(names.len(), steps + 1, "{names:?}");
        assert!(names.last().unwrap().contains("Saturation"), "{names:?}");
        assert!(!edit.drag_targeted(10., true), "the drag has ended");

        // A neutral grey moves nothing; a sample of an edit since changed is dropped.
        let grey = TargetSample {
            tone: 0.5,
            mixer: [0.3, 0.3, 0.3],
            color: [0.6, 0., 0.],
        };
        let now = edit.recipe().clone();
        assert!(!edit.start_targeted(TargetKind::Hue, &grey, &now));
        assert!(
            !edit.start_targeted(TargetKind::Hue, &skin, &sampled),
            "stale sample"
        );
    }

    #[test]
    fn paste_from_previous_takes_the_photo_left_last() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        // A second photo beside the first.
        let other = path.with_file_name("other.dng");
        std::fs::copy(&path, &other).unwrap();
        library.add_new_photos().unwrap();
        let other_id = library
            .photos()
            .unwrap()
            .iter()
            .find(|p| p.id != id)
            .unwrap()
            .id;

        let mut first = open(&library, id, &path);
        let exposure = Control::Param { index: 2 };
        first.set(exposure, 0.8, true);
        let mut second = open(&library, other_id, &other);
        assert_eq!(second.paste_previous(), None, "no photo left yet");
        first.leave();
        let wanted = first.recipe().exposure;
        assert!(second.paste_previous().is_some());
        assert_eq!(second.recipe().exposure, wanted);
        assert_eq!(second.state().steps.last().unwrap(), "Paste from Previous");
        assert_eq!(second.paste_previous(), None, "nothing more to change");
    }

    #[test]
    fn levels_splits_and_point_curves_are_steps_that_keep_the_edit_valid() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let steps = edit.state().steps.len();
        // The black point can't pass the white point.
        edit.set(Control::Levels { which: 1 }, 0.4, true);
        edit.set(Control::Levels { which: 0 }, 0.9, true);
        assert!((edit.recipe().black_point - 0.3899).abs() < 1e-5);
        edit.recipe().validate().unwrap();
        // A split drag is one step and stays apart from its neighbour.
        for (t, done) in [(0.45, false), (0.6, false), (0.9, true)] {
            edit.set_parametric_split(0, Some(t), done);
        }
        assert_eq!(edit.recipe().effects.splits[0], 0.45);
        // Strong Contrast from the menu, by its index.
        let menu = crate::tone::menu(edit.recipe());
        let strong = menu
            .names
            .iter()
            .position(|n| n == "Strong Contrast")
            .unwrap();
        assert!(edit.choose_point_curve(strong));
        assert!(!edit.choose_point_curve(strong), "already that curve");
        assert_eq!(crate::tone::menu(edit.recipe()).shown, "Strong Contrast");
        let names = edit.state().steps;
        assert_eq!(names.len(), steps + 4, "{names:?}");
        assert_eq!(names.last().unwrap(), "Point Curve Strong Contrast");
        edit.recipe().validate().unwrap();
    }

    #[test]
    fn a_grading_wheel_drag_is_one_step_named_by_what_moved() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let steps = edit.state().steps.len();
        // Straight up, then out past the rim: a quarter turn, full saturation.
        edit.set_grade_wheel(0, Some([0., -0.3]), false);
        edit.set_grade_wheel(0, Some([0., -0.6]), false);
        edit.set_grade_wheel(0, Some([0., -1.4]), true);
        assert_eq!(edit.recipe().grading[0][..2], [0.25, 1.]);
        let state = edit.state();
        assert_eq!(state.steps.len(), steps + 1, "one step for the drag");
        assert!(
            state
                .steps
                .last()
                .unwrap()
                .starts_with("Shadow Hue & Saturation")
        );

        // Out along the same hue changes saturation alone; Global is its own wheel.
        edit.set_grade_wheel(3, Some([0., -0.5]), true);
        edit.set_grade_wheel(3, Some([0., -0.2]), true);
        assert_eq!(edit.recipe().effects.global_grade[..2], [0.25, 0.2]);
        assert!(
            edit.state()
                .steps
                .last()
                .unwrap()
                .starts_with("Global Saturation")
        );
        edit.step(-1);
        assert_eq!(edit.recipe().effects.global_grade[1], 0.5);

        // Reset takes hue and saturation off, keeping luminance; again, nothing.
        edit.set(
            Control::Grade {
                region: 0,
                channel: 2,
            },
            0.3,
            true,
        );
        assert!(edit.set_grade_wheel(0, None, true));
        assert_eq!(edit.recipe().grading[0], [0., 0., 0.3]);
        assert!(!edit.set_grade_wheel(0, None, true));
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
