//! The Tone Curve's parametric graph and Point Curve menu, as the desktop's:
//! the curve as it renders ([`rawmakase_engine::develop::parametric::samples`]),
//! its region splits (kept apart by [`Effects::move_split`]), which region a
//! point of the graph falls in, and Lightroom's built-in curves with the
//! curves saved beside them ([`rawmakase_interop::presets::curves`]).

use rawmakase_engine::develop::effects::EffectsRendering;
use rawmakase_interop::presets::curves::{BuiltinCurve, PointCurve, SavedCurves, ShownCurve};
use rawmakase_model::model::effects::Effects;
use rawmakase_model::model::params::ParameterId;

/// The parametric curve as the graph draws it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct ParametricCurve {
    /// The curve's output at evenly spaced inputs from 0 to 1, as it renders.
    pub samples: Vec<f32>,
    /// Where the Shadows, Darks, Lights and Highlights regions meet, 0–1.
    pub splits: Vec<f32>,
}

/// Points along the graph the curve is drawn through.
const SAMPLES: usize = 128;

pub(crate) fn parametric(e: &Effects) -> ParametricCurve {
    ParametricCurve {
        samples: rawmakase_engine::develop::parametric::samples(e, SAMPLES),
        splits: e.splits.to_vec(),
    }
}

/// The slider of the region the graph's input `x` (0–1) falls in, which a
/// drag up or down there moves, as on the desktop.
pub(crate) fn region_control(e: &Effects, x: f32) -> Option<crate::controls::Control> {
    let id = ParameterId::PARAMETRIC.get(e.parametric_region(x.clamp(0., 1.)))?;
    Some(crate::controls::Control::Param {
        index: crate::controls::index_of(*id)?,
    })
}

/// The Point Curve menu: Lightroom's curves, then the ones saved here.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PointCurveMenu {
    /// Every curve the menu offers, built-in ones first: what
    /// [`crate::LiveView::choose_point_curve`] takes, by index.
    pub names: Vec<String>,
    /// How many of `names` are Lightroom's built-in curves.
    pub builtin: u32,
    /// The curve the photo has ("Custom" for one of its own).
    pub shown: String,
}

pub(crate) fn menu(r: &rawmakase_model::model::recipe::Recipe) -> PointCurveMenu {
    let saved = SavedCurves::default().list().curves;
    PointCurveMenu {
        names: BuiltinCurve::ALL
            .iter()
            .map(|c| c.name().to_string())
            .chain(saved.iter().map(|s| s.name.clone()))
            .collect(),
        builtin: BuiltinCurve::ALL.len() as u32,
        shown: ShownCurve::of(r, &saved).name(&saved).to_string(),
    }
}

/// Gives `r` the menu's curve `index`; returns its name, or `None` for no
/// such curve.
pub(crate) fn choose(
    r: &mut rawmakase_model::model::recipe::Recipe,
    index: usize,
) -> Option<String> {
    if let Some(curve) = BuiltinCurve::ALL.get(index) {
        curve.apply(r);
        return Some(curve.name().into());
    }
    let saved = SavedCurves::default().list().curves;
    let curve = saved.get(index - BuiltinCurve::ALL.len())?;
    curve.curve.apply(r);
    Some(curve.name.clone())
}

/// Saves `r`'s point curves under `name`, as the desktop's Save… does.
pub(crate) fn save(r: &rawmakase_model::model::recipe::Recipe, name: &str) -> anyhow::Result<()> {
    SavedCurves::default().save(name, &PointCurve::of(r))?;
    Ok(())
}
