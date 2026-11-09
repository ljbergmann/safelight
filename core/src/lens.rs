//! Lens Corrections' profile choice, as the desktop's Make, Model and
//! Profile menus offer it: the imported Adobe lens profiles that fit the
//! photo's camera, the one in use, and Auto. The rules are the model's
//! ([`LensProfileChoice`]).

use rawmakase_model::camera_data::Metadata;
use rawmakase_model::lens::choice::{LensProfileChoice, LensProfileSetup};
use rawmakase_model::model::recipe::Recipe;

/// An imported lens profile that fits the photo's camera.
#[derive(uniffi::Record, Clone, Debug)]
pub struct LensProfileInfo {
    pub index: u32,
    /// Its `ProfileName`, as Lightroom's Profile menu shows it.
    pub name: String,
    /// The lens it profiles, "Make Model".
    pub lens: String,
    /// Whether it profiles this photo's lens.
    pub fits: bool,
    /// Whether the photo renders with it now.
    pub used: bool,
}

/// The photo's choices, those that profile its lens first.
pub(crate) fn profiles(r: &Recipe, m: &Metadata) -> Vec<LensProfileInfo> {
    let resolved = r.lens_profile_choice.resolve(&m.lens_profiles, m);
    let mut list: Vec<LensProfileInfo> = m
        .lens_profiles
        .all()
        .iter()
        .enumerate()
        .map(|(i, c)| LensProfileInfo {
            index: i as u32,
            name: c.profile.name.clone(),
            lens: format!("{} {}", c.profile.lens_make, c.profile.lens_model)
                .trim()
                .to_string(),
            fits: c.lens_rank.is_some(),
            used: resolved.used.is_some_and(|u| std::ptr::eq(u, c)),
        })
        .collect();
    list.sort_by_key(|p| !p.fits);
    list
}

/// Chooses profile `index` (of [`profiles`]' indices), or with `None` Auto:
/// the imported profile that fits the lens best. The History step's name and
/// value; `None` when there is no such profile.
pub(crate) fn choose(r: &mut Recipe, m: &Metadata, index: Option<u32>) -> Option<(String, String)> {
    match index {
        Some(i) => {
            let candidate = m.lens_profiles.all().get(i as usize)?;
            r.lens_profile_choice.choose(&candidate.profile);
            r.lens_profile = true;
            Some(("Lens Profile".into(), candidate.profile.name.clone()))
        }
        None => {
            r.lens_profile_choice = LensProfileChoice {
                setup: LensProfileSetup::Auto,
                id: None,
            };
            Some(("Lens Profile".into(), "Auto".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Auto clears a picked profile, as Lightroom's Setup does; an index
    /// with no profile changes nothing. (A pick itself needs an imported
    /// Adobe profile, which can't be shipped as a fixture.)
    #[test]
    fn auto_clears_a_pick_and_a_missing_profile_is_refused() {
        let m = Metadata::default();
        let mut r = Recipe::default();
        assert_eq!(choose(&mut r, &m, Some(0)), None);
        assert_eq!(r.lens_profile_choice, LensProfileChoice::default());
        assert_eq!(
            choose(&mut r, &m, None),
            Some(("Lens Profile".into(), "Auto".into()))
        );
        assert_eq!(r.lens_profile_choice.setup, LensProfileSetup::Auto);
        assert!(profiles(&r, &m).is_empty());
    }
}
