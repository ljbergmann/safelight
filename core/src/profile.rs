//! The Profile browser: the profiles a photo can use, laid out as the
//! desktop's Profile menu lays them out ([`Groups::entry`]: camera profiles
//! and Adobe's at the top level, each group of looks on its own), and
//! choosing one as the desktop does (a new profile starts at 100%, and white
//! balance and the camera's exposure follow it).

use rawmakase_engine::develop::black_white::TreatmentChoice;
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::camera_profiles::{CameraProfile, Groups};
use rawmakase_model::model::recipe::Recipe;
use std::sync::Arc;

/// The camera matrix with the DNG's default look, which a RAW renders through
/// without a profile.
pub(crate) const MATRIX: &str = "Default (camera matrix)";

/// A profile the browser lists.
#[derive(uniffi::Record)]
pub struct ProfileInfo {
    /// What [`crate::LiveView::set_profile`] takes; `None` is the camera
    /// matrix.
    pub index: Option<u32>,
    pub title: String,
    /// The group of looks it belongs to; empty at the top level.
    pub group: String,
    pub selected: bool,
    /// Whether Profile Amount applies to it.
    pub has_amount: bool,
}

/// Every profile `metadata`'s photo can use, top level first, then each group
/// by name, as the menu orders them.
pub(crate) fn list(
    r: &Recipe,
    metadata: &Metadata,
    profiles: &[Arc<CameraProfile>],
    groups: &Groups,
) -> Vec<ProfileInfo> {
    let mut out = Vec::new();
    if !metadata.is_rendered() {
        out.push(ProfileInfo {
            index: None,
            title: MATRIX.into(),
            group: String::new(),
            selected: r.profile.is_none(),
            has_amount: false,
        });
    }
    let mut listed: Vec<ProfileInfo> = profiles
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let entry = groups.entry(p);
            ProfileInfo {
                index: Some(i as u32),
                title: entry.title,
                group: entry.group.unwrap_or_default(),
                selected: r.profile.as_ref() == Some(p),
                has_amount: p.supports_amount(),
            }
        })
        .collect();
    // The top level keeps the listing's order (by name); groups sort by name.
    listed.sort_by_key(|p| (!p.group.is_empty(), p.group.to_lowercase()));
    out.extend(listed);
    out
}

/// Chooses profile `index` of `profiles` (`None`: the camera matrix) for `r`;
/// returns its title for the History step, or `None` when nothing changes.
pub(crate) fn choose(
    r: &mut Recipe,
    metadata: &Metadata,
    profiles: &[Arc<CameraProfile>],
    groups: &Groups,
    index: Option<u32>,
) -> Option<String> {
    let chosen = match index {
        Some(i) => Some(profiles.get(i as usize)?.clone()),
        None if metadata.is_rendered() => return None,
        None => None,
    };
    if chosen == r.profile {
        return None;
    }
    let title = chosen
        .as_ref()
        .map_or_else(|| MATRIX.to_string(), |p| groups.entry(p).title);
    let old = r.profile.take();
    r.profile = chosen;
    // A newly chosen profile starts at 100%, as in Lightroom.
    r.profile_amount = 1.;
    r.profile_changed(metadata);
    // A black & white look switches the treatment, as the desktop's does;
    // the desktop also measures a first B&W mix from the photo, which the
    // iPad doesn't yet, so the mix starts neutral.
    r.follow_profile_treatment(old.as_deref(), None);
    Some(title)
}

/// What importing profiles or presets did, for the app to say.
#[derive(uniffi::Record, Debug)]
pub struct ImportSummary {
    /// Profiles, looks, LUTs or presets added, or found added already.
    pub done: u32,
    /// Files that couldn't be imported.
    pub failed: u32,
    /// One line: "Imported 3 profiles · 1 already imported".
    pub message: String,
    /// A line for each LUT renamed or held approximately, and each file that
    /// failed, with why.
    pub details: Vec<String>,
}

/// Imports camera profiles (DCP), looks (XMP) and `.cube` LUTs from
/// `paths`, files or folders, into the profile library, as the desktop's
/// Import Profiles does. LUTs are taken as expecting sRGB, the desktop's
/// default. A photo open in the editor lists them after
/// [`crate::LiveView::reload_profiles`].
#[uniffi::export]
pub fn import_profiles(paths: Vec<String>) -> ImportSummary {
    import_into(
        &paths,
        &rawmakase_model::storage::data_dir().join("camera-profiles"),
    )
}

pub(crate) fn import_into(paths: &[String], destination: &std::path::Path) -> ImportSummary {
    let picks: Vec<std::path::PathBuf> = paths.iter().map(Into::into).collect();
    let summary = rawmakase_interop::bulk_import::import_profiles_into(
        &picks,
        rawmakase_model::camera_profiles::LutSpace::default(),
        destination,
    );
    ImportSummary::from(&summary)
}

impl From<&rawmakase_interop::bulk_import::Summary> for ImportSummary {
    fn from(summary: &rawmakase_interop::bulk_import::Summary) -> Self {
        Self {
            done: summary.done() as u32,
            failed: summary.failures() as u32,
            message: summary.message(),
            details: summary.lines(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2×2×2 `.cube` that keeps every colour.
    const IDENTITY: &str =
        "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
    const PRESET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+0.50"/></rdf:RDF></x:xmpmeta>"#;

    #[test]
    fn a_lut_becomes_a_look_once_and_a_preset_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let picked = dir.path().join("Picked");
        std::fs::create_dir_all(&picked).unwrap();
        std::fs::write(picked.join("Teal.cube"), IDENTITY).unwrap();
        std::fs::write(picked.join("Warm.xmp"), PRESET).unwrap();
        let library = dir.path().join("camera-profiles");
        let paths = vec![
            picked.join("Teal.cube").display().to_string(),
            picked.join("Warm.xmp").display().to_string(),
        ];

        let first = import_into(&paths, &library);
        assert_eq!((first.done, first.failed), (1, 1), "{first:?}");
        assert!(
            first.details.iter().any(|l| l.starts_with("Warm.xmp")),
            "{first:?}"
        );
        let files: Vec<_> = std::fs::read_dir(&library).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "the LUT's look");

        // Again: found imported already, not a second look.
        let again = import_into(&paths[..1], &library);
        assert_eq!((again.done, again.failed), (1, 0), "{again:?}");
        assert_eq!(std::fs::read_dir(&library).unwrap().count(), 1);
    }

    #[test]
    fn presets_are_imported_with_their_folder_once() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("Pack");
        std::fs::create_dir_all(pack.join("Warm")).unwrap();
        std::fs::write(pack.join("Warm/Glow.xmp"), PRESET).unwrap();
        let (presets, profiles) = (dir.path().join("presets"), dir.path().join("profiles"));
        let import = || {
            ImportSummary::from(&rawmakase_interop::bulk_import::import_presets_into(
                &[pack.clone()],
                &presets,
                &profiles,
            ))
        };
        let first = import();
        assert_eq!((first.done, first.failed), (1, 0), "{first:?}");
        assert!(
            presets.join("Imported/Pack/Warm/Glow.xmp").exists(),
            "kept in its folder"
        );
        let again = import();
        assert_eq!(
            (again.done, again.failed),
            (1, 0),
            "found imported already: {again:?}"
        );
        assert!(again.message.contains("already"), "{again:?}");
    }
}
