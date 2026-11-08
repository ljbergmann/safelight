//! The Profile browser: the profiles a photo can use, laid out as the
//! desktop's Profile menu lays them out ([`Groups::entry`]: camera profiles
//! and Adobe's at the top level, each group of looks on its own), and
//! choosing one as the desktop does (a new profile starts at 100%, and white
//! balance and the camera's exposure follow it).

use rawmakase_model::camera_data::Metadata;
use rawmakase_model::camera_profiles::{CameraProfile, Groups};
use rawmakase_engine::develop::black_white::TreatmentChoice;
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
