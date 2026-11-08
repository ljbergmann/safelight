//! Lightroom's Before/After views: Before alone, or beside the edit left and
//! right or top and bottom, or split across one photo.
//!
//! Before is the open photo's, not saved with the edit, as in Lightroom and
//! the desktop: the photo's starting settings (its raw defaults) until the
//! edit, a History state or a snapshot is copied to it. It never takes
//! edits; framing it like the edit and the Copy and Swap commands are
//! RAWmakase's ([`rawmakase_edit_session::before_after`]). Where the two sides
//! go on screen is [`panes`], which the live view draws with and the app maps
//! touches with.

use crate::edit::OpenEdit;
use rawmakase_edit_session::before_after::{self, Transfer};
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::recipe::Recipe;

/// How the editor shows Before.
#[derive(uniffi::Enum, Clone, Copy, Debug, Default, PartialEq)]
pub enum Compare {
    /// The edit alone.
    #[default]
    Off,
    /// Before alone, full frame.
    BeforeOnly,
    /// Before and the edit side by side, each the whole photo: Before on
    /// the left, or on top.
    SideBySide { top_bottom: bool },
    /// One photo, Before on the left of (or above) a line at `position`
    /// (0–1 across the view) and the edit on the other side.
    Split { top_bottom: bool, position: f32 },
}

/// The Before/After commands, as the desktop's menu offers them.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeforeAfterCopy {
    /// Copy Before Settings to After: one History step.
    BeforeToAfter,
    /// Copy After Settings to Before.
    AfterToBefore,
    /// Swap Before and After Settings: the edit's change is one History step.
    Swap,
}

/// Where a side goes: `area`, the rectangle its photo is fitted or zoomed
/// in, and `clip`, the part of the view it shows in (half of the area in a
/// split view). `[x, y, width, height]`, in the units of the view's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pane {
    pub(crate) area: [f32; 4],
    pub(crate) clip: [f32; 4],
}

/// Both sides of a view: the edit always, Before when it shows beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Panes {
    pub(crate) after: Pane,
    pub(crate) before: Option<Pane>,
}

/// The space between the two sides of a side-by-side view, in pixels.
const GAP: f32 = 16.;

/// Where the edit is laid out in a view `width` × `height` pixels large for
/// `compare`, as `[x, y, width, height]`: the rectangle a touch on the edit
/// maps through.
#[uniffi::export]
pub fn after_area(compare: Compare, width: f32, height: f32) -> Vec<f32> {
    panes(compare, width, height).after.area.to_vec()
}

/// Lays out a view `width` × `height` pixels for `compare`.
pub(crate) fn panes(compare: Compare, width: f32, height: f32) -> Panes {
    let whole = [0., 0., width, height];
    let pane = |area: [f32; 4]| Pane { area, clip: area };
    match compare {
        Compare::Off | Compare::BeforeOnly => Panes {
            after: pane(whole),
            before: None,
        },
        Compare::SideBySide { top_bottom: false } => {
            let w = ((width - GAP) / 2.).max(1.);
            Panes {
                before: Some(pane([0., 0., w, height])),
                after: pane([width - w, 0., w, height]),
            }
        }
        Compare::SideBySide { top_bottom: true } => {
            let h = ((height - GAP) / 2.).max(1.);
            Panes {
                before: Some(pane([0., 0., width, h])),
                after: pane([0., height - h, width, h]),
            }
        }
        Compare::Split {
            top_bottom,
            position,
        } => {
            let at = position.clamp(0., 1.);
            let (before, after) = if top_bottom {
                let line = (height * at).round();
                ([0., 0., width, line], [0., line, width, height - line])
            } else {
                let line = (width * at).round();
                ([0., 0., line, height], [line, 0., width - line, height])
            };
            Panes {
                before: Some(Pane {
                    area: whole,
                    clip: before,
                }),
                after: Pane {
                    area: whole,
                    clip: after,
                },
            }
        }
    }
}

impl OpenEdit {
    /// The settings Before shows: those copied to it, else the photo's
    /// starting settings, framed as the edit is.
    pub(crate) fn before_settings(&self) -> Recipe {
        let before = self.before.clone().unwrap_or_else(|| {
            rawmakase_interop::raw_defaults::DevelopDefaults::default()
                .resolve(&self.metadata, &self.profiles)
                .recipe
        });
        before_after::framed_like(before, self.session.recipe())
    }

    /// Copies or swaps settings between Before and the edit; returns whether
    /// either changed.
    pub(crate) fn transfer(&mut self, copy: BeforeAfterCopy) -> bool {
        self.session.finish_gesture();
        let transfer = match copy {
            BeforeAfterCopy::BeforeToAfter => Transfer::BeforeToAfter,
            BeforeAfterCopy::AfterToBefore => Transfer::AfterToBefore,
            BeforeAfterCopy::Swap => Transfer::Swap,
        };
        let done = before_after::transfer(transfer, self.before_settings(), self.session.recipe());
        let changed = done.before.is_some() || done.edit.is_some();
        if let Some(before) = done.before {
            self.before = Some(before);
        }
        if let Some((step, edit)) = done.edit {
            // As the desktop: set as a whole, then recorded as one step.
            let was = self.session.replace(edit);
            self.session.commit(was, Some(step));
        }
        changed
    }

    /// History's Copy History Step Settings to Before: the state with
    /// `applied` steps applied.
    pub(crate) fn before_from_history(&mut self, applied: usize) -> bool {
        self.session.finish_gesture();
        let state = self.session.history().state(applied, self.session.recipe());
        let found = state.is_some();
        if found {
            self.before = state;
        }
        found
    }

    /// Copy Snapshot Settings to Before.
    pub(crate) fn before_from_snapshot(
        &mut self,
        id: i64,
        image: Option<&CameraImage>,
    ) -> anyhow::Result<Vec<String>> {
        let (recipe, skipped) = self.snapshot_settings(id, image)?;
        self.before = Some(recipe);
        Ok(skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::Control;
    use crate::edit::tests::{exposure, library, open};

    #[test]
    fn before_starts_as_the_photo_opened_new_framed_like_the_edit() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        let fresh = edit.recipe().clone();
        edit.set(Control::Param { index: 2 }, 1., true);
        edit.set_crop([0.1, 0.2, 0.8, 0.9], true);
        let before = edit.before_settings();
        assert_eq!(before.exposure, fresh.exposure);
        assert_eq!(before.crop, [0.1, 0.2, 0.8, 0.9], "framed like the edit");
    }

    #[test]
    fn copying_and_swapping_between_before_and_the_edit() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Control::Param { index: 2 }, 1., true);

        // After to Before records nothing in History.
        assert!(edit.transfer(BeforeAfterCopy::AfterToBefore));
        assert_eq!(edit.before_settings().exposure, 1.);
        assert_eq!(edit.state().steps.len(), 1);

        edit.set(Control::Param { index: 2 }, -0.5, true);
        assert!(edit.transfer(BeforeAfterCopy::Swap));
        assert_eq!(exposure(&edit), 1.);
        assert_eq!(edit.before_settings().exposure, -0.5);
        assert_eq!(
            edit.state().steps.last().unwrap(),
            "Swap Before and After Settings"
        );

        assert!(edit.transfer(BeforeAfterCopy::BeforeToAfter));
        assert_eq!(exposure(&edit), -0.5);
        assert_eq!(
            edit.state().steps.last().unwrap(),
            "Copy Before Settings to After"
        );
        let steps = edit.state().steps.len();
        assert!(
            !edit.transfer(BeforeAfterCopy::BeforeToAfter),
            "already the same"
        );
        assert_eq!(edit.state().steps.len(), steps);

        // A History state, then a snapshot, copied to Before.
        assert!(edit.before_from_history(0));
        assert_eq!(edit.before_settings().exposure, 0.);
        edit.set(Control::Param { index: 2 }, 0.3, true);
        let snapshot = edit.add_snapshot(None).unwrap();
        edit.set(Control::Param { index: 2 }, 0.9, true);
        edit.before_from_snapshot(snapshot, None).unwrap();
        assert_eq!(edit.before_settings().exposure, 0.3);
        assert_eq!(exposure(&edit), 0.9, "the edit is untouched");
    }

    #[test]
    fn side_by_side_views_halve_the_view_and_split_views_share_it() {
        let side = panes(Compare::SideBySide { top_bottom: false }, 1016., 600.);
        assert_eq!(side.before.unwrap().area, [0., 0., 500., 600.]);
        assert_eq!(side.after.area, [516., 0., 500., 600.]);
        let stacked = panes(Compare::SideBySide { top_bottom: true }, 800., 616.);
        assert_eq!(stacked.after.area, [0., 316., 800., 300.]);

        let split = panes(
            Compare::Split {
                top_bottom: false,
                position: 0.25,
            },
            800.,
            600.,
        );
        let before = split.before.unwrap();
        assert_eq!(
            (before.area, split.after.area),
            ([0., 0., 800., 600.], [0., 0., 800., 600.])
        );
        assert_eq!(
            (before.clip, split.after.clip),
            ([0., 0., 200., 600.], [200., 0., 600., 600.])
        );
        assert_eq!(panes(Compare::BeforeOnly, 800., 600.).before, None);
    }
}
