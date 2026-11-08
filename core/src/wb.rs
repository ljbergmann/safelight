//! White balance presets, the eyedropper and Auto, as the desktop's Basic
//! panel offers them: As Shot, Auto and Lightroom's named presets (for RAW
//! files only), a tap on something neutral, and Auto Settings for the tone
//! sliders. The estimates are the engine's ([`auto_tone`],
//! [`auto_white_balance`], [`neutral_pick`]); what Auto takes from an
//! estimate, and what an estimate depends on, follow the desktop's
//! `app::develop::auto`.

use rawmakase_cancel::Cancel;
use rawmakase_engine::develop::{
    AutoTone, auto_tone, auto_tone_basis, auto_white_balance, neutral_pick,
};
use rawmakase_model::camera_data::{CameraImage, Metadata};
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::white_balance::NamedWhiteBalance;

pub(crate) const AS_SHOT: &str = "As Shot";
pub(crate) const AUTO: &str = "Auto";
const CUSTOM: &str = "Custom";

/// What the white balance menu offers for this photo: As Shot and Auto, and
/// the named presets for a RAW whose profile knows the light's temperature.
/// Empty for a monochrome photo, which has no white balance.
pub(crate) fn choices(r: &Recipe, m: &Metadata) -> Vec<String> {
    if m.monochrome {
        return vec![];
    }
    let mut out = vec![AS_SHOT.to_string(), AUTO.to_string()];
    if !m.is_rendered() && !uncalibrated(r) {
        out.extend(NamedWhiteBalance::ALL.iter().map(|w| w.name().to_string()));
    }
    out
}

/// A fitted Camera Look reads As Shot as D65 whatever the light, so named
/// temperatures would be wrong for it, as the desktop says.
fn uncalibrated(r: &Recipe) -> bool {
    r.profile
        .as_ref()
        .is_some_and(|p| rawmakase_model::camera_profiles::fitted::is_uncalibrated_base(p))
}

/// The menu's current choice: As Shot, Auto, a named preset, or Custom.
pub(crate) fn current(r: &Recipe, m: &Metadata) -> String {
    let rendered = m.is_rendered();
    let now = if rendered {
        (r.incremental_temperature, r.incremental_tint)
    } else {
        (r.temperature, r.tint)
    };
    let near = |(t, n): (f32, f32)| (t - now.0).abs() < 1. && (n - now.1).abs() < 0.5;
    let as_shot = if rendered {
        (0., 0.)
    } else {
        let mut shot = r.clone();
        shot.reset_white_balance(m);
        (shot.temperature, shot.tint)
    };
    if r.auto_white_balance.is_some_and(|[t, n]| near((t, n))) {
        AUTO.into()
    } else if near(as_shot) {
        AS_SHOT.into()
    } else if rendered {
        CUSTOM.into()
    } else {
        NamedWhiteBalance::ALL
            .into_iter()
            .find(|w| near((w.values().temperature, w.values().tint)))
            .map_or(CUSTOM, NamedWhiteBalance::name)
            .into()
    }
}

/// Sets As Shot or a named preset; `false` for a name this photo doesn't
/// offer (Auto runs as [`estimate`]).
pub(crate) fn set_named(r: &mut Recipe, m: &Metadata, name: &str) -> bool {
    if !choices(r, m).iter().any(|c| c == name) {
        return false;
    }
    if name == AS_SHOT {
        r.wb = [1.; 3];
        r.reset_white_balance(m);
        r.auto_white_balance = None;
        return true;
    }
    let Some(named) = NamedWhiteBalance::from_name(name) else {
        return false;
    };
    let values = named.values();
    r.temperature = values.temperature;
    r.tint = values.tint;
    r.update_wb(m);
    r.auto_white_balance = None;
    true
}

/// The eyedropper: makes the point at `u`, `v` of the photo as shown (0–1,
/// cropped) neutral.
pub(crate) fn pick(im: &CameraImage, r: &mut Recipe, u: f32, v: f32) {
    let gains = neutral_pick(im, r, u, v);
    r.pick_white_balance(gains, &im.metadata);
    r.auto_white_balance = None;
}

/// Which Auto.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoKind {
    /// Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Vibrance.
    Settings,
    WhiteBalance,
}

impl AutoKind {
    /// The History step, as the desktop names it.
    pub(crate) fn step(self) -> (&'static str, &'static str) {
        match self {
            Self::Settings => ("Auto Settings", ""),
            Self::WhiteBalance => ("White Balance", AUTO),
        }
    }
}

/// Everything an estimate of `kind` depends on: `r` without the settings it
/// chooses. An estimate made for other inputs no longer fits.
pub(crate) fn inputs(kind: AutoKind, r: &Recipe) -> Recipe {
    match kind {
        AutoKind::Settings => auto_tone_basis(r),
        AutoKind::WhiteBalance => Recipe {
            wb: [1.; 3],
            temperature: 0.,
            tint: 0.,
            incremental_temperature: 0.,
            incremental_tint: 0.,
            auto_white_balance: None,
            ..r.clone()
        },
    }
}

/// The engine's estimate for `base`: slow (a reduced render), so run it off
/// the edit's lock.
pub(crate) fn estimate(kind: AutoKind, im: &CameraImage, base: &Recipe) -> anyhow::Result<Recipe> {
    match kind {
        AutoKind::Settings => auto_tone(im, base, Cancel::NEVER),
        AutoKind::WhiteBalance => auto_white_balance(im, base, Cancel::NEVER),
    }
}

/// Takes from `auto` only the settings Auto chooses, keeping the rest of `r`.
pub(crate) fn apply(kind: AutoKind, auto: &Recipe, r: &mut Recipe) {
    match kind {
        AutoKind::Settings => AutoTone::of(auto).apply(r),
        AutoKind::WhiteBalance => {
            r.wb = auto.wb;
            r.temperature = auto.temperature;
            r.tint = auto.tint;
            r.incremental_temperature = auto.incremental_temperature;
            r.incremental_tint = auto.incremental_tint;
            r.auto_white_balance = auto.auto_white_balance;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawmakase_model::camera_data::Decode;
    use std::path::Path;

    fn chart() -> CameraImage {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        rawmakase_decode::photo::open(&path)
            .unwrap()
            .develop(Decode::Half, Cancel::NEVER)
            .unwrap()
    }

    #[test]
    fn auto_takes_only_what_it_chooses() {
        let im = chart();
        let mut r = Recipe::for_metadata(&im.metadata);
        r.effects.clarity = 0.3;
        r.exposure = -2.;
        let auto = estimate(AutoKind::Settings, &im, &r).unwrap();
        let mut applied = r.clone();
        apply(AutoKind::Settings, &auto, &mut applied);
        assert_ne!(applied.exposure, -2., "Auto set Exposure");
        assert_eq!(applied.effects.clarity, 0.3, "Clarity isn't Auto's");
        // Exposure is Auto's own setting: it is not one of its inputs.
        assert_eq!(
            inputs(AutoKind::Settings, &r),
            inputs(AutoKind::Settings, &applied)
        );

        let wb = estimate(AutoKind::WhiteBalance, &im, &r).unwrap();
        let mut balanced = r.clone();
        apply(AutoKind::WhiteBalance, &wb, &mut balanced);
        assert!(balanced.auto_white_balance.is_some());
        assert_eq!(current(&balanced, &im.metadata), AUTO);
        assert_eq!(balanced.exposure, -2.);
    }

    #[test]
    fn picking_a_grey_patch_keeps_white_balance_custom() {
        let im = chart();
        let mut r = Recipe::for_metadata(&im.metadata);
        pick(&im, &mut r, 0.5, 0.5);
        assert!(r.wb.iter().all(|g| g.is_finite() && *g > 0.));
        assert!(r.auto_white_balance.is_none());
        r.validate().unwrap();
    }
}
