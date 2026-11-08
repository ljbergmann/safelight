//! The Tone Curve's point curves: RGB, Red, Green and Blue, as the desktop's
//! panel edits them, through the model's own [`ToneCurve`] operations, so a
//! curve always has 2 to 32 points with increasing inputs. Lightroom's
//! built-in curves (Linear, Medium and Strong Contrast) set the RGB curve.

use rawmakase_interop::presets::curves::BuiltinCurve;
use rawmakase_model::color::curve::ToneCurve;
use rawmakase_model::model::recipe::Recipe;

/// Points sampled across a curve for drawing it.
const SAMPLES: usize = 65;

/// A point curve as the editor draws it.
#[derive(uniffi::Record)]
pub struct CurveState {
    /// 0 RGB, 1 Red, 2 Green, 3 Blue.
    pub channel: u32,
    /// x, y, x, y… from 0 to 1, by increasing x.
    pub points: Vec<f32>,
    /// The curve's output at 65 evenly spaced inputs from 0 to 1.
    pub samples: Vec<f32>,
    /// The built-in curve the RGB curve is, or "Custom".
    pub name: String,
}

pub(crate) const CHANNELS: [&str; 4] = ["RGB", "Red", "Green", "Blue"];

pub(crate) fn curve(r: &Recipe, channel: u32) -> Option<&ToneCurve> {
    match channel {
        0 => Some(&r.curve),
        1..=3 => r.effects.channels.get(channel as usize - 1),
        _ => None,
    }
}

pub(crate) fn curve_mut(r: &mut Recipe, channel: u32) -> Option<&mut ToneCurve> {
    match channel {
        0 => Some(&mut r.curve),
        1..=3 => r.effects.channels.get_mut(channel as usize - 1),
        _ => None,
    }
}

pub(crate) fn state(r: &Recipe, channel: u32) -> Option<CurveState> {
    let c = curve(r, channel)?;
    Some(CurveState {
        channel,
        points: c.points.iter().flatten().copied().collect(),
        samples: (0..SAMPLES)
            .map(|i| c.evaluate(i as f32 / (SAMPLES - 1) as f32))
            .collect(),
        name: builtin_name(r).unwrap_or("Custom").into(),
    })
}

/// The built-in curve the RGB curve is.
fn builtin_name(r: &Recipe) -> Option<&'static str> {
    BuiltinCurve::ALL
        .into_iter()
        .find(|b| b.curve().points == r.curve.points)
        .map(BuiltinCurve::name)
}

/// Lightroom's built-in curves' names.
pub(crate) fn builtin_names() -> Vec<String> {
    BuiltinCurve::ALL.iter().map(|b| b.name().to_string()).collect()
}

/// Sets the RGB curve to a built-in one; `false` for an unknown name.
pub(crate) fn set_builtin(r: &mut Recipe, name: &str) -> bool {
    let Some(b) = BuiltinCurve::ALL.into_iter().find(|b| b.name() == name) else {
        return false;
    };
    b.apply(r);
    true
}
