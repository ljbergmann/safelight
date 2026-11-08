//! Lightroom's Targeted Adjustment Tool, as the desktop's: a drag up or down
//! on the photo moves the sliders that change the colour (or tone) where it
//! started. Which sliders, by how much, and the History step's name are the
//! engine's ([`rawmakase_engine::develop::targeted`]); this keeps a drag's
//! state between its start and its end.

use rawmakase_engine::develop::targeted::{HslChannel, Target, TargetSample, TargetWeights};
use rawmakase_model::model::recipe::Recipe;

/// What the tool adjusts.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// The parametric curve's regions.
    ToneCurve,
    /// The Color Mixer's Hue, Saturation or Luminance sliders.
    Hue,
    Saturation,
    Luminance,
    /// The black & white mix.
    BlackWhite,
}

impl TargetKind {
    pub(crate) fn target(self) -> Target {
        match self {
            Self::ToneCurve => Target::ToneCurve,
            Self::Hue => Target::Hsl(HslChannel::Hue),
            Self::Saturation => Target::Hsl(HslChannel::Saturation),
            Self::Luminance => Target::Hsl(HslChannel::Luminance),
            Self::BlackWhite => Target::BlackWhite,
        }
    }
}

/// A drag with the tool, once its sample is in.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetDrag {
    /// The edit when it started, which the sliders move from.
    pub(crate) start: Recipe,
    /// The edit as the drag last left it: anything else changing it ends the drag.
    pub(crate) last: Recipe,
    pub(crate) weights: TargetWeights,
}

impl TargetDrag {
    /// A drag over a pixel `sample` of the edit `r`; `None` for a colour the
    /// tool leaves alone (a neutral, or black).
    pub(crate) fn new(kind: TargetKind, sample: &TargetSample, r: &Recipe) -> Option<Self> {
        let weights = TargetWeights::new(kind.target(), sample, r);
        (!weights.is_empty()).then(|| Self {
            start: r.clone(),
            last: r.clone(),
            weights,
        })
    }
}
