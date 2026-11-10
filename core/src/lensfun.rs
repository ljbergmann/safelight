//! Lensfun's community lens profiles in Lens Corrections, as the desktop
//! offers them where no imported Adobe profile fits: the download first (on
//! request only, with its terms), then the lens this photo matches, a choice
//! when the name fits several or none, and resolving or updating a lens an
//! edit names. Choosing one is an edit like any other, one History step.
//!
//! What is offered mirrors the desktop's Lensfun card
//! (`src/app/develop/panels/inspector/lensfun.rs`, egui-bound there); the
//! matching, the choice and the install are RAWmakase's ([`matching`],
//! [`LensfunChoice`], [`crate::lensfun_install`]). The database lives where
//! RAWmakase's crates read it ([`installed::dir`]), so presets and pasted
//! settings resolve against it too.

use crate::lensfun_install::{Installed as Done, Progress, Source};
use crate::render::RenderError;
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::lens::lensfun::choice::{camera_for, frame_size, photo_of};
use rawmakase_model::lens::lensfun::installed::{self, Installed};
use rawmakase_model::lens::lensfun::matching::{self, Outcome};
use rawmakase_model::lens::lensfun::{Lens, LensfunChoice};
use rawmakase_model::model::recipe::{ProfileCorrections, Recipe};
use rawmakase_model::optics::lensfun::Components;

/// What Lens Corrections offers from Lensfun for the open photo.
#[derive(uniffi::Record, Clone, Debug, Default, PartialEq)]
pub struct LensfunOffer {
    /// Nothing to show when empty.
    pub title: String,
    pub note: String,
    pub action: LensfunAction,
    /// The lenses [`LensfunAction::Choose`] picks from, by database index.
    pub lenses: Vec<LensfunLens>,
    /// The Lensfun lens the edit uses, if any.
    pub chosen: Option<String>,
    /// The installed database's date, if any.
    pub installed: Option<String>,
    /// The database's terms, shown beside the download.
    pub terms: String,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, Default, PartialEq)]
pub enum LensfunAction {
    #[default]
    None,
    /// Download Lensfun's database ([`install`]).
    Download,
    /// Use the lens this photo matches.
    Use { index: u32 },
    /// Pick one of [`LensfunOffer::lenses`].
    Choose,
    /// Resolve the lens the edit names against the installed database.
    Resolve,
    /// Take the installed database's newer calibration of the edit's lens.
    Update,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct LensfunLens {
    pub index: u32,
    pub label: String,
}

/// The installed database, if any (cached by RAWmakase until it changes).
pub(crate) fn current() -> Option<std::sync::Arc<Installed>> {
    installed::current()
}

const TERMS: &str = "Lensfun's profiles are made by Lensfun's contributors and shared under CC BY-SA 3.0, downloaded from lensfun.github.io.";

/// What to offer photo `m` edited as `r`, with `db` installed.
pub(crate) fn offer(r: &Recipe, m: &Metadata, db: Option<&Installed>) -> LensfunOffer {
    let mut out = LensfunOffer {
        installed: db.map(|d| d.manifest.date()),
        chosen: r
            .lens_profile_choice
            .lensfun()
            .map(|c| c.name().to_string()),
        ..LensfunOffer::default()
    };
    if m.is_rendered() {
        return out;
    }
    let mut say = |title: &str, note: &str, action| {
        out.title = title.into();
        out.note = note.into();
        out.action = action;
    };
    if let Some(chosen) = r.lens_profile_choice.lensfun() {
        let found = db.and_then(|d| d.db.find(&chosen.lens).map(|l| (d, l)));
        if !chosen.is_resolved() {
            match (db, found) {
                (None, _) => say(
                    chosen.name(),
                    "This edit names a Lensfun profile: download Lensfun's profiles to apply it.",
                    LensfunAction::Download,
                ),
                (Some(_), Some(_)) => say(
                    chosen.name(),
                    "Not applied yet: resolve it against the installed profiles.",
                    LensfunAction::Resolve,
                ),
                (Some(_), None) => say(
                    chosen.name(),
                    "Not in the installed Lensfun profiles; a newer version may have it.",
                    LensfunAction::None,
                ),
            }
        } else if let Some((d, lens)) = found
            && chosen.calibration.as_deref() != Some(&lens.calibration)
            && d.db.timestamp > chosen.provenance.database
        {
            say(
                "Newer calibration",
                &format!(
                    "Lensfun's version of {} has a newer calibration of this lens.",
                    d.manifest.date()
                ),
                LensfunAction::Update,
            );
        }
        if out.action == LensfunAction::Download {
            out.terms = TERMS.into();
        }
        return out;
    }
    // Only where no imported Adobe profile fits this lens, and the camera's
    // own corrections leave something to correct.
    if m.lens_profiles.auto(m).is_some() {
        return out;
    }
    let builtin = [m.lens.as_ref(), m.lens_stand_in.as_ref()]
        .into_iter()
        .flatten()
        .map(Components::of)
        .fold(Components::default(), |a, b| Components {
            distortion: a.distortion || b.distortion,
            chromatic: a.chromatic || b.chromatic,
            vignetting: a.vignetting || b.vignetting,
        });
    if builtin.distortion && builtin.chromatic && builtin.vignetting {
        return out;
    }
    let Some(db) = db else {
        say(
            "Community lens profiles",
            "Lensfun's free database may have a profile for this lens. Nothing changes until you choose one.",
            LensfunAction::Download,
        );
        out.terms = TERMS.into();
        return out;
    };
    let photo = photo_of(m, frame_size(m));
    let matched = matching::find(&db.db, &photo);
    match &matched.outcome {
        Outcome::Unique(i) => {
            let lens = &db.db.lenses[*i];
            if let Some(why) = lens.refusal() {
                say(
                    &lens_name(lens, m),
                    &format!("Can't be applied: Lensfun's profile is {why}."),
                    LensfunAction::None,
                );
                return out;
            }
            let gives = given(lens, db, m);
            let adds = Components {
                distortion: gives.distortion && !builtin.distortion,
                chromatic: gives.chromatic && !builtin.chromatic,
                vignetting: gives.vignetting && !builtin.vignetting,
            };
            if adds.any() {
                say(
                    &lens_name(lens, m),
                    &format!("Corrects {}.", gives.describe()),
                    LensfunAction::Use { index: *i as u32 },
                );
            }
        }
        Outcome::Ambiguous(all) => {
            let lenses: Vec<usize> = all.clone();
            say(
                &format!("{} possible lenses", lenses.len()),
                "The lens name fits more than one profile: choose the one used.",
                LensfunAction::Choose,
            );
            out.lenses = listed(db, &lenses);
        }
        Outcome::UnknownCamera if matched.crop.is_none() => say(
            "Unknown camera",
            "Lensfun doesn't know this camera, and the photo records no 35 mm equivalent focal length to tell its crop factor.",
            LensfunAction::None,
        ),
        Outcome::NoMatch | Outcome::NoLensName | Outcome::UnknownCamera => {
            let (title, note) = match matched.outcome {
                Outcome::UnknownCamera => (
                    "Unknown camera",
                    "Lensfun doesn't know this camera; choose the lens used.",
                ),
                Outcome::NoLensName => (
                    "Lens not named",
                    "The photo doesn't name its lens; choose the lens used.",
                ),
                _ => (
                    "No matching profile",
                    "No profile matched this lens's name; choose the lens used.",
                ),
            };
            let lenses = choices(db, m);
            if lenses.is_empty() {
                say(
                    title,
                    "Lensfun has no profile for this camera's mount and sensor size.",
                    LensfunAction::None,
                );
            } else {
                say(title, note, LensfunAction::Choose);
                out.lenses = listed(db, &lenses);
            }
        }
    }
    out
}

/// The lenses a photo may choose from: those fitting its camera and frame.
fn choices(db: &Installed, m: &Metadata) -> Vec<usize> {
    let photo = photo_of(m, frame_size(m));
    matching::choices(&db.db, &photo)
        .1
        .into_iter()
        .filter(|&i| db.db.lenses[i].refusal().is_none())
        .collect()
}

fn listed(db: &Installed, lenses: &[usize]) -> Vec<LensfunLens> {
    let all: Vec<&Lens> = lenses.iter().map(|&i| &db.db.lenses[i]).collect();
    lenses
        .iter()
        .map(|&i| {
            let lens = &db.db.lenses[i];
            LensfunLens {
                index: i as u32,
                label: format!("{} · {}", lens.label(), profile_label(lens, &all)),
            }
        })
        .collect()
}

/// "Lensfun", with its calibration when the lens has several, as the
/// desktop's menus name it.
fn profile_label(lens: &Lens, all: &[&Lens]) -> String {
    let same = all
        .iter()
        .filter(|l| l.maker.id == lens.maker.id && l.model.id == lens.model.id)
        .count();
    if same <= 1 {
        return "Lensfun".into();
    }
    let aspect = [
        ("3:2", 1.5f32),
        ("4:3", 4. / 3.),
        ("16:9", 16. / 9.),
        ("1:1", 1.),
    ]
    .iter()
    .find(|(_, ratio)| (lens.calibration.aspect - ratio).abs() < 0.01)
    .map(|(name, _)| name.to_string())
    .unwrap_or_else(|| format!("{:.2}:1", lens.calibration.aspect));
    format!("Lensfun (crop {:.2}, {aspect})", lens.calibration.crop)
}

/// How a Lensfun lens is named: a compact's "fixed lens" by its camera.
fn lens_name(lens: &Lens, m: &Metadata) -> String {
    let label = lens.label();
    if label.eq_ignore_ascii_case("fixed lens") && !m.model.is_empty() {
        format!("{} {} lens", m.make, m.model).trim().to_string()
    } else {
        label
    }
}

/// The corrections `lens` gives photo `m`.
fn given(lens: &Lens, db: &Installed, m: &Metadata) -> Components {
    LensfunChoice::of(lens, &db.db, camera_for(&db.db, m))
        .apply(m)
        .correction
        .as_ref()
        .map(Components::of)
        .unwrap_or_default()
}

/// Chooses database lens `index` for photo `m`: its calibration copied into
/// the edit, Enable Profile Corrections on. The History step's name and
/// value; `None` for an index the database doesn't have or can't apply.
pub(crate) fn choose(
    r: &mut Recipe,
    m: &Metadata,
    db: &Installed,
    index: u32,
) -> Option<(String, String)> {
    let lens = db.db.lenses.get(index as usize)?;
    if lens.refusal().is_some() {
        return None;
    }
    r.set_profile_corrections(m, ProfileCorrections::On);
    r.lens_profile_choice
        .choose_lensfun(LensfunChoice::of(lens, &db.db, camera_for(&db.db, m)));
    Some(("Lens Profile".into(), lens_name(lens, m)))
}

/// Resolves or updates the Lensfun lens the edit names against `db`.
pub(crate) fn resolve(r: &mut Recipe, m: &Metadata, db: &Installed) -> Option<(String, String)> {
    let resolved = r.lens_profile_choice.lensfun()?.resolved(&db.db, m)?;
    let name = resolved.name().to_string();
    r.lens_profile_choice.choose_lensfun(resolved);
    Some(("Lens Profile".into(), name))
}

/// Downloads and installs Lensfun's database from its update channel, or
/// says it was current. Takes a few seconds: call it off the main thread.
pub(crate) fn install() -> Result<String, RenderError> {
    let failed = |e: anyhow::Error| RenderError::Failed {
        message: format!("{e:#}"),
    };
    let dir = installed::dir();
    std::fs::create_dir_all(&dir).map_err(|e| failed(e.into()))?;
    let done = crate::lensfun_install::install(&dir, &Source::Lensfun, &Progress::default())
        .map_err(failed)?;
    Ok(match done {
        Done::New(m) => format!("Installed Lensfun's profiles of {}", m.date()),
        Done::Current(m) => format!("Lensfun's profiles of {} are current", m.date()),
    })
}

/// Removes the installed database. Edits keep their lens's calibration.
pub(crate) fn remove() -> Result<bool, RenderError> {
    crate::lensfun_install::remove(&installed::dir(), &Progress::default()).map_err(|e| {
        RenderError::Failed {
            message: format!("{e:#}"),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawmakase_model::lens::lensfun::fixture;

    fn db() -> std::sync::Arc<Installed> {
        fixture::installed(-0.02, 1_793_180_205).get().unwrap()
    }

    /// Without the database, a photo no imported profile fits is offered
    /// the download with its terms; nothing changes in the edit.
    #[test]
    fn without_the_database_the_download_is_offered() {
        let o = offer(&Recipe::default(), &fixture::photo(), None);
        assert_eq!(o.action, LensfunAction::Download);
        assert!(o.terms.contains("CC BY-SA"));
        assert_eq!(o.chosen, None);
    }

    /// With it, the matched lens is offered and Use copies its calibration,
    /// one step named after the lens; the offer then says nothing more.
    #[test]
    fn the_matched_lens_is_used_and_kept_in_the_edit() {
        let (db, m) = (db(), fixture::photo());
        let mut r = Recipe::default();
        let o = offer(&r, &m, Some(&db));
        let LensfunAction::Use { index } = o.action else {
            panic!("{o:?}");
        };
        assert!(o.note.starts_with("Corrects "), "{}", o.note);
        let (name, value) = choose(&mut r, &m, &db, index).unwrap();
        assert_eq!(name, "Lens Profile");
        assert!(value.contains("70-200"), "{value}");
        let chosen = r.lens_profile_choice.lensfun().unwrap();
        assert!(chosen.is_resolved());
        let after = offer(&r, &m, Some(&db));
        assert_eq!(after.action, LensfunAction::None);
        assert!(after.chosen.unwrap().contains("70-200"));
        r.validate().unwrap();
        assert!(choose(&mut r, &m, &db, 99).is_none());
    }

    /// A lens chosen from an older database is offered the newer
    /// calibration, and Update takes it.
    #[test]
    fn a_newer_calibration_is_offered_and_taken() {
        let m = fixture::photo();
        let mut r = Recipe::default();
        r.lens_profile_choice.choose_lensfun(fixture::choice());
        let newer = fixture::installed(-0.05, 1_800_000_000).get().unwrap();
        assert_eq!(offer(&r, &m, Some(&newer)).action, LensfunAction::Update);
        assert!(resolve(&mut r, &m, &newer).is_some());
        assert_eq!(offer(&r, &m, Some(&newer)).action, LensfunAction::None);
    }
}
