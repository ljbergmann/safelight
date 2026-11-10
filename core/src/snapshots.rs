//! Lightroom's Snapshots: named states of the open photo's edit, kept in the
//! catalog as the desktop keeps them ([`Catalog::add_snapshot`] and the rest),
//! so a snapshot made on the iPad shows in RAWmakase on a computer too.
//! Applying one is a History step; snapshots brought from Lightroom hold
//! Lightroom's settings and are converted when applied, as on the desktop.
//!
//! [`Catalog::add_snapshot`]: rawmakase_catalog::catalog::Catalog::add_snapshot

use crate::edit::OpenEdit;
use crate::edit_session::history::Step;
use anyhow::{Context, Result};
use rawmakase_catalog::catalog::{PhotoId, Snapshot, SnapshotSettings};
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::recipe::Recipe;

/// A snapshot in the list, alphabetically as Lightroom lists them.
#[derive(uniffi::Record)]
pub struct SnapshotInfo {
    pub id: i64,
    pub name: String,
}

impl OpenEdit {
    fn stored_snapshots(&self) -> Result<Vec<Snapshot>> {
        self.library.catalog().snapshots(PhotoId(self.id))
    }

    /// This photo's snapshot `id`; another photo's is refused, so a list
    /// shown for the last photo can't change this one's.
    fn snapshot(&self, id: i64) -> Result<Snapshot> {
        self.stored_snapshots()?
            .into_iter()
            .find(|s| s.id == id)
            .context("This photo has no such snapshot")
    }

    pub(crate) fn snapshots(&self) -> Result<Vec<SnapshotInfo>> {
        Ok(self
            .stored_snapshots()?
            .into_iter()
            .map(|s| SnapshotInfo {
                id: s.id,
                name: s.name,
            })
            .collect())
    }

    /// Saves the edit as it is now as a snapshot named `name`, or "Snapshot
    /// N" as the desktop's + names it; returns its id.
    pub(crate) fn add_snapshot(&mut self, name: Option<&str>) -> Result<i64> {
        self.session.finish_gesture();
        let name = match name.map(str::trim) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => format!("Snapshot {}", self.stored_snapshots()?.len() + 1),
        };
        self.library
            .catalog()
            .add_snapshot(PhotoId(self.id), &name, self.session.recipe())
    }

    /// Lightroom's Update with Current Settings.
    pub(crate) fn update_snapshot(&mut self, id: i64) -> Result<()> {
        self.session.finish_gesture();
        self.snapshot(id)?;
        self.library
            .catalog()
            .update_snapshot(id, self.session.recipe())
    }

    /// Renames a snapshot; an emptied name keeps the old one, as on the
    /// desktop.
    pub(crate) fn rename_snapshot(&self, id: i64, name: &str) -> Result<()> {
        self.snapshot(id)?;
        match name.trim() {
            "" => Ok(()),
            name => self.library.catalog().rename_snapshot(id, name),
        }
    }

    pub(crate) fn delete_snapshot(&self, id: i64) -> Result<()> {
        self.snapshot(id)?;
        self.library.catalog().delete_snapshot(id)
    }

    /// A snapshot's settings for this photo; one from Lightroom is converted
    /// with `image` for its Auto settings. Also returns what Lightroom set
    /// that doesn't render here.
    pub(crate) fn snapshot_settings(
        &self,
        id: i64,
        image: Option<&CameraImage>,
    ) -> Result<(Recipe, Vec<String>)> {
        let (recipe, skipped) = match self.snapshot(id)?.settings {
            SnapshotSettings::Recipe(recipe) => (*recipe, Vec::new()),
            SnapshotSettings::Lightroom(text) => {
                let measures = image.map(rawmakase_engine::develop::Measures);
                rawmakase_interop::lr_develop::convert_develop(
                    &text,
                    &self.metadata,
                    &self.profiles,
                    measures
                        .as_ref()
                        .map(|m| m as &dyn rawmakase_interop::xmp::PhotoMeasures),
                )?
            }
        };
        recipe.validate()?;
        Ok((recipe, skipped))
    }

    /// Applies a snapshot as one History step, "Snapshot: <name>", as
    /// Lightroom does; nothing when it is already the edit. Returns what a
    /// Lightroom snapshot set that doesn't render here.
    pub(crate) fn apply_snapshot(
        &mut self,
        id: i64,
        image: Option<&CameraImage>,
    ) -> Result<Vec<String>> {
        self.session.finish_gesture();
        let name = self.snapshot(id)?.name;
        let (recipe, skipped) = self.snapshot_settings(id, image)?;
        if recipe != *self.session.recipe() {
            // As the desktop: set as a whole, which counts a change of masks,
            // then recorded as one step.
            let before = self.session.replace(recipe);
            self.session
                .commit(before, Some(Step::new(format!("Snapshot: {name}"), "")));
        }
        Ok(skipped)
    }
}

#[cfg(test)]
mod tests {
    use crate::controls::Control;
    use crate::edit::tests::{exposure, library, open};

    #[test]
    fn a_snapshot_saves_names_and_restores_the_edit_as_a_history_step() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Control::Param { index: 2 }, 0.8, true);

        // + saves the edit as it is, named as the desktop names it.
        let snapshot = edit.add_snapshot(None).unwrap();
        let listed = edit.snapshots().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Snapshot 1");
        edit.rename_snapshot(snapshot, "Bright").unwrap();
        edit.rename_snapshot(snapshot, "  ").unwrap();
        assert_eq!(
            edit.snapshots().unwrap()[0].name,
            "Bright",
            "an emptied name keeps the old one"
        );

        // Applying it is one History step, which Undo takes back.
        edit.set(Control::Param { index: 2 }, -1., true);
        edit.apply_snapshot(snapshot, None).unwrap();
        assert_eq!(exposure(&edit), 0.8);
        assert_eq!(edit.state().steps.last().unwrap(), "Snapshot: Bright");
        let steps = edit.state().steps.len();
        edit.apply_snapshot(snapshot, None).unwrap();
        assert_eq!(edit.state().steps.len(), steps, "already the edit: no step");
        edit.step(-1);
        assert_eq!(exposure(&edit), -1.);

        // Update with Current Settings, then Delete.
        edit.update_snapshot(snapshot).unwrap();
        edit.step(1);
        edit.apply_snapshot(snapshot, None).unwrap();
        assert_eq!(exposure(&edit), -1.);
        edit.delete_snapshot(snapshot).unwrap();
        assert!(edit.snapshots().unwrap().is_empty());
        assert!(edit.apply_snapshot(snapshot, None).is_err());
    }

    #[test]
    fn snapshots_belong_to_their_photo_and_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let (library, id, path) = library(dir.path());
        let mut edit = open(&library, id, &path);
        edit.set(Control::Param { index: 2 }, 0.5, true);
        let snapshot = edit.add_snapshot(Some("Half a stop")).unwrap();
        edit.save().unwrap();
        drop(edit);
        let reopened = open(&library, id, &path);
        let listed = reopened.snapshots().unwrap();
        assert_eq!(
            (listed[0].id, listed[0].name.as_str()),
            (snapshot, "Half a stop")
        );
    }
}
