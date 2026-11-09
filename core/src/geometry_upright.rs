//! Upright as the Transform panel offers it: its modes' names and what its
//! analysis measures, as the desktop's.

use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::transform::{LensInputs, UprightMode};

/// What an Upright analysis measures: the photo's orientation and its lens
/// correction. Corrections analysed under other inputs don't fit. Mirrors
/// the desktop's `upright::inputs`.
pub(crate) fn inputs(r: &Recipe) -> (u8, bool, bool, LensInputs) {
    (r.rotation, r.flip_x, r.flip_y, LensInputs::of(r))
}

/// The mode's name, as the Transform panel shows it.
pub(crate) fn name(mode: UprightMode) -> &'static str {
    match mode {
        UprightMode::Off => "Off",
        UprightMode::Auto => "Auto",
        UprightMode::Full => "Full",
        UprightMode::Level => "Level",
        UprightMode::Vertical => "Vertical",
        UprightMode::Guided => "Guided",
    }
}

/// Upright's modes the iPad offers; Guided needs its guides drawn first.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upright {
    Off,
    Auto,
    Level,
    Vertical,
    Full,
}

impl Upright {
    pub(crate) fn mode(self) -> UprightMode {
        match self {
            Upright::Off => UprightMode::Off,
            Upright::Auto => UprightMode::Auto,
            Upright::Level => UprightMode::Level,
            Upright::Vertical => UprightMode::Vertical,
            Upright::Full => UprightMode::Full,
        }
    }

    pub(crate) fn of(mode: UprightMode) -> Option<Self> {
        Some(match mode {
            UprightMode::Off => Upright::Off,
            UprightMode::Auto => Upright::Auto,
            UprightMode::Level => Upright::Level,
            UprightMode::Vertical => Upright::Vertical,
            UprightMode::Full => Upright::Full,
            UprightMode::Guided => return None,
        })
    }
}
