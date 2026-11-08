//! Every Develop slider the iPad shows, laid out as the desktop's panels are,
//! read from and written to the recipe by the rules RAWmakase's model keeps.
//!
//! Most controls are the model's develop parameters ([`ParameterId`]): their
//! label, range, drag step and value text come from its descriptors, and they
//! change through [`change_setting`], which keeps the side effects (white
//! balance multipliers after Temp or Tint, Auto Straighten after an angle).
//! The Color Mixer, B&W mix and Color Grading wheels are recipe fields with no
//! descriptor; their ranges are the ones `Recipe::validate` accepts, and the
//! desktop's panels write them directly, as these do.

use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::edit::change_setting;
use rawmakase_model::model::masks::MaskShape;
use rawmakase_model::model::panels::{Panel, PanelState};
use rawmakase_model::model::params::ParameterId;
use rawmakase_model::model::recipe::Recipe;

/// One slider: which setting it moves.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// A develop parameter, by its index in [`PARAMS`].
    Param { index: u32 },
    /// Color Mixer: band 0..8 (red to magenta), channel 0..3 (hue, saturation,
    /// luminance).
    Hsl { band: u32, channel: u32 },
    /// B&W Mix: band 0..8.
    Gray { band: u32 },
    /// Color Grading: region 0..4 (shadows, midtones, highlights, global),
    /// channel 0..3 (hue, saturation, luminance).
    Grade { region: u32, channel: u32 },
    /// A mask's adjustment: mask index, and slider index in
    /// [`crate::masks::LOCAL`].
    Mask { mask: u32, index: u32 },
    /// Lightroom's Profile Amount, for a profile that has one (a look).
    ProfileAmount,
    /// A mask's Amount, which scales all its sliders (0–200%).
    MaskAmount { mask: u32 },
    /// Levels' black point (`which` 0) or white point (1), 0–1, kept at
    /// least 0.01 apart as the desktop keeps them.
    Levels { which: u32 },
    /// A Point Color swatch's setting: `which` 0 to 2 its Hue, Saturation
    /// and Luminance shifts, 3 its Variance, 4 its Range.
    PointColor { swatch: u32, which: u32 },
    /// A range part's setting: Color Range's Refine (`which` 0); Luminance
    /// Range's low, high, and the falloff below and above (0 to 3).
    Range {
        mask: u32,
        component: u32,
        which: u32,
    },
}

/// The range part `component` of mask `mask`.
fn range_shape(r: &Recipe, mask: u32, component: u32) -> Option<&MaskShape> {
    let shape = &r
        .masks
        .get(mask as usize)?
        .components
        .get(component as usize)?
        .shape;
    shape.is_range().then_some(shape)
}

fn range_value(shape: &mut MaskShape, which: u32) -> Option<&mut f32> {
    match (shape, which) {
        (MaskShape::ColorRange { amount, .. }, 0) => Some(amount),
        (MaskShape::LuminanceRange { low, .. }, 0) => Some(low),
        (MaskShape::LuminanceRange { high, .. }, 1) => Some(high),
        (MaskShape::LuminanceRange { falloff, .. }, 2) => Some(&mut falloff[0]),
        (MaskShape::LuminanceRange { falloff, .. }, 3) => Some(&mut falloff[1]),
        _ => None,
    }
}

/// An on/off setting a panel shows above its sliders.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// Whether a panel's settings apply, by its index in [`Panel::ALL`].
    Panel {
        index: u32,
    },
    /// Black & White treatment.
    Monochrome,
    /// Lens Corrections: the lens profile, and chromatic aberration removal.
    LensProfile,
    LensChromaticAberration,
    /// AI Denoise; switching it on sets manual noise reduction to 0, as on
    /// the desktop.
    AiDenoise,
}

#[derive(uniffi::Record, Clone)]
pub struct SliderState {
    pub control: Control,
    pub label: String,
    /// In the recipe's units, within `min`..`max`.
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// What dragging snaps to; 0 moves freely.
    pub step: f32,
    /// The value as the desktop shows it.
    pub text: String,
    /// What a double tap sets it back to: its value in the photo's starting
    /// settings (Temp and Tint as shot), a new mask's for a mask's slider.
    pub reset: f32,
}

#[derive(uniffi::Record)]
pub struct SwitchState {
    pub id: Switch,
    pub label: String,
    pub on: bool,
}

/// A panel, or one view of it (the Color Mixer's Hue, say).
#[derive(uniffi::Record)]
pub struct Section {
    /// The group it shows in: Profile, Light, Color, Effects, Detail, Optics
    /// or Geometry (the Crop tool's).
    pub group: String,
    pub title: String,
    pub switches: Vec<SwitchState>,
    pub sliders: Vec<SliderState>,
}

/// The develop parameters the panels show, in one list so a [`Control`] can
/// name one by index.
pub const PARAMS: [ParameterId; 58] = [
    // Basic
    ParameterId::Temperature,
    ParameterId::Tint,
    ParameterId::Exposure,
    ParameterId::Contrast,
    ParameterId::Highlights,
    ParameterId::Shadows,
    ParameterId::Whites,
    ParameterId::Blacks,
    ParameterId::Texture,
    ParameterId::Clarity,
    ParameterId::Dehaze,
    ParameterId::Vibrance,
    ParameterId::Saturation,
    // Tone Curve
    ParameterId::ParametricHighlights,
    ParameterId::ParametricLights,
    ParameterId::ParametricDarks,
    ParameterId::ParametricShadows,
    ParameterId::Midtone,
    ParameterId::CurveSaturation,
    // Color Grading
    ParameterId::GradingBlending,
    ParameterId::GradingBalance,
    // Detail
    ParameterId::SharpeningAmount,
    ParameterId::SharpeningRadius,
    ParameterId::SharpeningDetail,
    ParameterId::SharpeningMasking,
    ParameterId::LuminanceNoise,
    ParameterId::LuminanceDetail,
    ParameterId::LuminanceContrast,
    ParameterId::ColorNoise,
    ParameterId::ColorNoiseDetail,
    ParameterId::ColorNoiseSmoothness,
    // Lens Corrections
    ParameterId::LensDistortion,
    ParameterId::LensVignetting,
    ParameterId::ManualDistortion,
    ParameterId::LensVignetteAmount,
    ParameterId::LensVignetteMidpoint,
    // Transform
    ParameterId::TransformVertical,
    ParameterId::TransformHorizontal,
    ParameterId::TransformRotate,
    ParameterId::TransformAspect,
    ParameterId::TransformScale,
    ParameterId::TransformOffsetX,
    ParameterId::TransformOffsetY,
    // Effects
    ParameterId::VignetteAmount,
    ParameterId::VignetteMidpoint,
    ParameterId::VignetteFeather,
    ParameterId::GrainAmount,
    ParameterId::GrainSize,
    ParameterId::GrainRoughness,
    // Calibration
    ParameterId::ShadowTint,
    ParameterId::RedPrimaryHue,
    ParameterId::RedPrimarySaturation,
    ParameterId::GreenPrimaryHue,
    ParameterId::GreenPrimarySaturation,
    ParameterId::BluePrimaryHue,
    ParameterId::BluePrimarySaturation,
    // Crop & Straighten (appended, so the indices above stay as they are)
    ParameterId::Straighten,
    ParameterId::AiDenoiseAmount,
];

const BANDS: [&str; 8] = [
    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
];
const CHANNELS: [&str; 3] = ["Hue", "Saturation", "Luminance"];
pub(crate) const REGIONS: [(&str, &str); 4] = [
    ("Shadows", "Shadow"),
    ("Midtones", "Midtone"),
    ("Highlights", "Highlight"),
    ("Global", "Global"),
];

pub(crate) fn index_of(id: ParameterId) -> Option<u32> {
    PARAMS.iter().position(|p| *p == id).map(|i| i as u32)
}

fn param_at(index: u32) -> Option<ParameterId> {
    PARAMS.get(index as usize).copied()
}

/// What a control shows: its label, range, step, and how it is written.
struct Spec {
    label: String,
    min: f32,
    max: f32,
    step: f32,
    /// Shown value = value × scale, with this many decimals and a sign.
    scale: f32,
    decimals: usize,
    signed: bool,
    /// Appended to the shown value ("°").
    unit: &'static str,
}

impl Control {
    fn spec(self, photo: &Metadata) -> Option<Spec> {
        match self {
            Control::Param { index } => {
                let id = param_at(index)?.for_photo(Some(photo));
                let d = id.descriptor();
                Some(Spec {
                    label: d.label.to_string(),
                    min: *d.interactive.start(),
                    max: *d.interactive.end(),
                    step: d.drag_step.unwrap_or(0.),
                    scale: d.display.scale,
                    decimals: d.display.decimals,
                    signed: d.display.signed,
                    unit: "",
                })
            }
            Control::Hsl { band, channel } => Some(Spec {
                label: format!(
                    "{} {}",
                    BANDS.get(band as usize)?,
                    CHANNELS.get(channel as usize)?
                ),
                min: -1.,
                max: 1.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: true,
                unit: "",
            }),
            Control::Gray { band } => Some(Spec {
                label: format!("{} Gray Mix", BANDS.get(band as usize)?),
                min: -1.,
                max: 1.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: true,
                unit: "",
            }),
            Control::Mask { index, .. } => {
                let d = crate::masks::LOCAL.get(index as usize)?.descriptor();
                Some(Spec {
                    label: d.label.to_string(),
                    min: *d.interactive.start(),
                    max: *d.interactive.end(),
                    step: d.drag_step.unwrap_or(0.),
                    scale: d.display.scale,
                    decimals: d.display.decimals,
                    signed: d.display.signed,
                    unit: "",
                })
            }
            Control::MaskAmount { .. } => Some(Spec {
                label: "Amount".into(),
                min: 0.,
                max: 2.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: false,
                unit: "",
            }),
            Control::Range { which, .. } => Some(Spec {
                label: match which {
                    0 => "Refine",
                    1 => "High",
                    2 => "Falloff Low",
                    _ => "Falloff High",
                }
                .into(),
                min: 0.,
                max: 1.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: false,
                unit: "",
            }),
            Control::Levels { which } => Some(Spec {
                label: if which == 0 {
                    "Black Point"
                } else {
                    "White Point"
                }
                .into(),
                min: 0.,
                max: 1.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: false,
                unit: "",
            }),
            Control::PointColor { which, .. } => Some(Spec {
                label: ["Hue Shift", "Sat Shift", "Lum Shift", "Variance", "Range"]
                    .get(which as usize)?
                    .to_string(),
                min: if which == 4 { 0. } else { -1. },
                max: 1.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: which != 4,
                unit: "",
            }),
            Control::ProfileAmount => Some(Spec {
                label: "Profile Amount".into(),
                min: 0.,
                max: 2.,
                step: 0.,
                scale: 100.,
                decimals: 0,
                signed: false,
                unit: "",
            }),
            Control::Grade { region, channel } => {
                let (_, step_word) = REGIONS.get(region as usize)?;
                let label = format!("{step_word} {}", CHANNELS.get(channel as usize)?);
                Some(match channel {
                    // Hue as a fraction of the circle, shown in degrees.
                    0 => Spec {
                        label,
                        min: 0.,
                        max: 1.,
                        step: 0.,
                        scale: 360.,
                        decimals: 0,
                        signed: false,
                        unit: "°",
                    },
                    1 => Spec {
                        label,
                        min: 0.,
                        max: 1.,
                        step: 0.,
                        scale: 100.,
                        decimals: 0,
                        signed: false,
                        unit: "",
                    },
                    _ => Spec {
                        label,
                        min: -1.,
                        max: 1.,
                        step: 0.,
                        scale: 100.,
                        decimals: 0,
                        signed: true,
                        unit: "",
                    },
                })
            }
        }
    }

    fn get(self, r: &Recipe, photo: &Metadata) -> Option<f32> {
        match self {
            Control::Param { index } => {
                let id = param_at(index)?.for_photo(Some(photo));
                let mut r = r.clone();
                Some(*id.value_mut(&mut r))
            }
            Control::Hsl { band, channel } => {
                r.hsl.get(band as usize)?.get(channel as usize).copied()
            }
            Control::Gray { band } => r.effects.gray_mix.get(band as usize).copied(),
            Control::Mask { mask, index } => {
                let mut adjust = r.masks.get(mask as usize)?.adjust;
                Some(
                    *crate::masks::LOCAL
                        .get(index as usize)?
                        .value_mut(&mut adjust),
                )
            }
            Control::Grade { region, channel } => {
                let grade = if region == 3 {
                    &r.effects.global_grade
                } else {
                    r.grading.get(region as usize)?
                };
                grade.get(channel as usize).copied()
            }
            Control::ProfileAmount => Some(r.profile_amount),
            Control::Levels { which } => Some(if which == 0 {
                r.black_point
            } else {
                r.white_point
            }),
            Control::MaskAmount { mask } => Some(r.masks.get(mask as usize)?.amount),
            Control::PointColor { swatch, which } => {
                let p = r.point_colors.get(swatch as usize)?;
                match which {
                    0..=2 => Some(p.shift[which as usize]),
                    3 => Some(p.variance),
                    4 => Some(p.range),
                    _ => None,
                }
            }
            Control::Range {
                mask,
                component,
                which,
            } => {
                let mut shape = range_shape(r, mask, component)?.clone();
                range_value(&mut shape, which).copied()
            }
        }
    }

    /// Sets the control to `value`, clamped to its range, the way the desktop
    /// does: a parameter through [`change_setting`], the others directly.
    /// Returns the label and value text its History step is named with.
    pub(crate) fn set(
        self,
        r: &mut Recipe,
        photo: &Metadata,
        value: f32,
    ) -> Option<(String, String)> {
        let spec = self.spec(photo)?;
        let value = value.clamp(spec.min, spec.max);
        match self {
            Control::Param { index } => {
                let id = param_at(index)?.for_photo(Some(photo));
                change_setting(r, id, Some(photo), |r| *id.value_mut(r) = value);
            }
            Control::Hsl { band, channel } => r.hsl[band as usize][channel as usize] = value,
            Control::Gray { band } => r.effects.gray_mix[band as usize] = value,
            Control::Mask { mask, index } => {
                let id = *crate::masks::LOCAL.get(index as usize)?;
                *id.value_mut(&mut r.masks.get_mut(mask as usize)?.adjust) = value;
            }
            Control::Grade { region, channel } => {
                let grade = if region == 3 {
                    &mut r.effects.global_grade
                } else {
                    &mut r.grading[region as usize]
                };
                grade[channel as usize] = value;
            }
            Control::MaskAmount { mask } => r.masks.get_mut(mask as usize)?.amount = value,
            Control::PointColor { swatch, which } => {
                let p = r.point_colors.get_mut(swatch as usize)?;
                match which {
                    0..=2 => p.shift[which as usize] = value,
                    3 => p.variance = value,
                    4 => p.range = value,
                    _ => return None,
                }
            }
            Control::Range {
                mask,
                component,
                which,
            } => {
                range_shape(r, mask, component)?;
                let shape = &mut r.masks[mask as usize].components[component as usize].shape;
                *range_value(shape, which)? = value;
                // Low stays at or under High.
                if let MaskShape::LuminanceRange { low, high, .. } = shape {
                    if which == 0 {
                        *high = high.max(*low);
                    } else if which == 1 {
                        *low = low.min(*high);
                    }
                }
            }
            Control::Levels { which } => {
                // A hair over the 0.01 the model asks for, so rounding never
                // leaves them closer.
                const APART: f32 = 0.0101;
                if which == 0 {
                    r.black_point = value.min(r.white_point - APART).max(0.);
                } else {
                    r.white_point = value.max(r.black_point + APART).min(1.);
                }
            }
            Control::ProfileAmount => {
                // A profile without an Amount renders at 100%.
                if !r.profile.as_ref().is_some_and(|p| p.supports_amount()) {
                    return None;
                }
                r.profile_amount = value;
            }
        }
        let shown = self.get(r, photo)?;
        Some((spec.label.clone(), spec.text(shown)))
    }

    /// Its slider in `r`, resetting to its value in `defaults` (see
    /// [`defaults_for`]).
    fn state(self, r: &Recipe, photo: &Metadata, defaults: &Recipe) -> Option<SliderState> {
        let spec = self.spec(photo)?;
        let value = self.get(r, photo)?;
        Some(SliderState {
            control: self,
            text: spec.text(value),
            label: spec.label,
            reset: match self {
                // A swatch resets to a newly sampled one's: no shifts or
                // Variance, Range 50.
                Control::PointColor { which, .. } => {
                    if which == 4 {
                        0.5
                    } else {
                        0.
                    }
                }
                _ => self.get(defaults, photo).unwrap_or(value),
            }
            .clamp(spec.min, spec.max),
            value,
            min: spec.min,
            max: spec.max,
            step: spec.step,
        })
    }
}

/// Control `control`'s slider in `r`, for a control with no default in the
/// photo's starting settings (a Point Color swatch's).
pub(crate) fn slider(control: Control, r: &Recipe, photo: &Metadata) -> Option<SliderState> {
    control.state(r, photo, r)
}

impl Spec {
    fn text(&self, value: f32) -> String {
        let shown = format!("{:.*}", self.decimals, value * self.scale);
        // "-0" reads as zero.
        let shown = if shown
            .trim_start_matches('-')
            .chars()
            .all(|c| c == '0' || c == '.')
        {
            shown.trim_start_matches('-').to_string()
        } else {
            shown
        };
        let sign = if self.signed && value * self.scale >= 0.5 / 10f32.powi(self.decimals as i32) {
            "+"
        } else {
            ""
        };
        format!("{sign}{shown}{}", self.unit)
    }
}

impl Switch {
    fn label(self) -> &'static str {
        match self {
            Switch::Panel { .. } => "On",
            Switch::Monochrome => "Black & White",
            Switch::LensProfile => "Enable Profile Corrections",
            Switch::LensChromaticAberration => "Remove Chromatic Aberration",
            Switch::AiDenoise => "AI Denoise",
        }
    }

    fn get(self, r: &Recipe) -> Option<bool> {
        Some(match self {
            Switch::Panel { index } => {
                r.panels.state(*Panel::ALL.get(index as usize)?) == PanelState::On
            }
            Switch::Monochrome => r.effects.monochrome,
            Switch::LensProfile => r.lens_profile,
            Switch::LensChromaticAberration => r.lens_ca,
            Switch::AiDenoise => r.ai_denoise.enabled,
        })
    }

    /// Turns the setting on or off; returns the History step's name and value.
    pub(crate) fn set(self, r: &mut Recipe, on: bool) -> Option<(String, String)> {
        let word = if on { "On" } else { "Off" };
        Some(match self {
            Switch::Panel { index } => {
                let panel = *Panel::ALL.get(index as usize)?;
                r.panels
                    .set(panel, if on { PanelState::On } else { PanelState::Off });
                (panel_title(panel).to_string(), word.to_string())
            }
            Switch::Monochrome => {
                r.effects.monochrome = on;
                (
                    "Treatment".into(),
                    if on { "Black & White" } else { "Color" }.into(),
                )
            }
            Switch::LensProfile => {
                r.lens_profile = on;
                ("Profile Corrections".into(), word.into())
            }
            Switch::LensChromaticAberration => {
                r.lens_ca = on;
                ("Remove Chromatic Aberration".into(), word.into())
            }
            Switch::AiDenoise => {
                r.switch_ai_denoise(on);
                ("AI Denoise".into(), word.into())
            }
        })
    }

    fn state(self, r: &Recipe) -> Option<SwitchState> {
        Some(SwitchState {
            id: self,
            label: self.label().to_string(),
            on: self.get(r)?,
        })
    }
}

fn panel_title(panel: Panel) -> &'static str {
    match panel {
        Panel::ToneCurve => "Tone Curve",
        Panel::ColorMixer => "Color Mixer",
        Panel::BlackWhiteMix => "B&W Mix",
        Panel::ColorGrading => "Color Grading",
        Panel::Detail => "Detail",
        Panel::LensCorrections => "Lens Corrections",
        Panel::Transform => "Transform",
        Panel::Effects => "Effects",
        Panel::Calibration => "Calibration",
        Panel::SpotRemoval => "Spot Removal",
        Panel::RedEye => "Red Eye",
        Panel::Masks => "Masks",
    }
}

fn panel_switch(panel: Panel) -> Switch {
    let index = Panel::ALL.iter().position(|p| *p == panel).unwrap_or(0) as u32;
    Switch::Panel { index }
}

fn params(ids: &[ParameterId]) -> Vec<Control> {
    ids.iter()
        .filter_map(|id| index_of(*id))
        .map(|index| Control::Param { index })
        .collect()
}

/// Mask `mask`'s adjustment sliders, in Lightroom's order.
pub(crate) fn mask_sliders(
    r: &Recipe,
    photo: &Metadata,
    mask: u32,
    start: &Recipe,
) -> Vec<SliderState> {
    let defaults = defaults_for(r, start);
    let Some(group) = r.masks.get(mask as usize) else {
        return vec![];
    };
    let mut controls = vec![Control::MaskAmount { mask }];
    for (i, c) in group.components.iter().enumerate() {
        let component = i as u32;
        let range = |which| Control::Range {
            mask,
            component,
            which,
        };
        match c.shape {
            MaskShape::ColorRange { .. } => controls.push(range(0)),
            MaskShape::LuminanceRange { .. } => controls.extend((0..4).map(range)),
            _ => {}
        }
    }
    controls
        .extend((0..crate::masks::LOCAL.len() as u32).map(|index| Control::Mask { mask, index }));
    controls
        .into_iter()
        .filter_map(|c| c.state(r, photo, &defaults))
        .map(|mut s| {
            // Luminance Range's first slider is Low.
            if let Control::Range {
                which: 0,
                mask,
                component,
            } = s.control
                && matches!(
                    range_shape(r, mask, component),
                    Some(MaskShape::LuminanceRange { .. })
                )
            {
                s.label = "Low".into();
            }
            s
        })
        .collect()
}

/// Mask `mask`'s sliders grouped as Lightroom for iPad's Masking panel
/// groups them: the mask itself (Amount, a range's sliders), then Light,
/// Color, Effects and Detail. RAWmakase's masks have no Optics.
pub(crate) fn mask_sections(
    r: &Recipe,
    photo: &Metadata,
    mask: u32,
    start: &Recipe,
) -> Vec<Section> {
    use rawmakase_model::model::params::LocalParameterId as L;
    let group_of = |c: &Control| -> &'static str {
        let Control::Mask { index, .. } = c else {
            return "Mask";
        };
        match crate::masks::LOCAL.get(*index as usize) {
            Some(
                L::Exposure | L::Contrast | L::Highlights | L::Shadows | L::Whites | L::Blacks,
            ) => "Light",
            Some(L::Texture | L::Clarity | L::Dehaze) => "Effects",
            Some(L::Sharpness | L::Noise) => "Detail",
            _ => "Color",
        }
    };
    let sliders = mask_sliders(r, photo, mask, start);
    ["Mask", "Light", "Color", "Effects", "Detail"]
        .into_iter()
        .map(|g| Section {
            group: g.to_string(),
            title: g.to_string(),
            switches: vec![],
            sliders: sliders
                .iter()
                .filter(|s| group_of(&s.control) == g)
                .cloned()
                .collect(),
        })
        .filter(|s| !s.sliders.is_empty())
        .collect()
}

/// What `r`'s sliders reset to: `start`, the photo's starting settings,
/// with `r`'s masks as new masks of the same shapes, so a mask's sliders
/// go back to a new mask's values.
pub(crate) fn defaults_for(r: &Recipe, start: &Recipe) -> Recipe {
    let fresh = rawmakase_model::model::masks::MaskGroup::default();
    let mut defaults = start.clone();
    defaults.masks = r
        .masks
        .iter()
        .map(|m| rawmakase_model::model::masks::MaskGroup {
            amount: fresh.amount,
            adjust: fresh.adjust,
            ..m.clone()
        })
        .collect();
    defaults
}

/// The panels, grouped as Lightroom for iPad groups its Edit panel, with
/// their current values: Light (tone, the curve), Color (white balance,
/// presence of colour, the mixer, grading, calibration), Effects, Detail
/// (AI Denoise, sharpening, noise reduction), Optics (lens corrections) and
/// Geometry (crop and transform, for the Crop tool). Every control of the
/// desktop's panels is in one of them, once.
pub(crate) fn sections(r: &Recipe, photo: &Metadata, start: &Recipe) -> Vec<Section> {
    let defaults = defaults_for(r, start);
    use ParameterId as P;
    let group = |g: &str, title: &str, switches: Vec<Switch>, controls: Vec<Control>| {
        (g.to_string(), title.to_string(), switches, controls)
    };
    let mut layout: Vec<(String, String, Vec<Switch>, Vec<Control>)> = Vec::new();
    // Profile Amount, beside the profile, for a profile that has one.
    if r.profile.as_ref().is_some_and(|p| p.supports_amount()) {
        layout.push(group(
            "Profile",
            "Profile",
            vec![],
            vec![Control::ProfileAmount],
        ));
    }
    layout.extend([
        group(
            "Light",
            "Light",
            vec![],
            params(&[
                P::Exposure,
                P::Contrast,
                P::Highlights,
                P::Shadows,
                P::Whites,
                P::Blacks,
            ]),
        ),
        group(
            "Light",
            "Tone Curve",
            vec![panel_switch(Panel::ToneCurve)],
            params(&[
                P::ParametricHighlights,
                P::ParametricLights,
                P::ParametricDarks,
                P::ParametricShadows,
                P::CurveSaturation,
            ]),
        ),
        group(
            "Light",
            "Tone Curve · Levels",
            vec![],
            [Control::Levels { which: 0 }, Control::Levels { which: 1 }]
                .into_iter()
                .chain(params(&[P::Midtone]))
                .collect(),
        ),
        group(
            "Color",
            "Color",
            vec![Switch::Monochrome],
            params(&[P::Temperature, P::Tint, P::Vibrance, P::Saturation]),
        ),
    ]);
    if r.effects.monochrome {
        layout.push(group(
            "Color",
            "B&W Mix",
            vec![panel_switch(Panel::BlackWhiteMix)],
            (0..8).map(|band| Control::Gray { band }).collect(),
        ));
    } else {
        for (channel, name) in CHANNELS.iter().enumerate() {
            layout.push(group(
                "Color",
                &format!("Color Mixer · {name}"),
                if channel == 0 {
                    vec![panel_switch(Panel::ColorMixer)]
                } else {
                    vec![]
                },
                (0..8)
                    .map(|band| Control::Hsl {
                        band,
                        channel: channel as u32,
                    })
                    .collect(),
            ));
        }
    }
    for (region, (title, _)) in REGIONS.iter().enumerate() {
        layout.push(group(
            "Color",
            &format!("Color Grading · {title}"),
            if region == 0 {
                vec![panel_switch(Panel::ColorGrading)]
            } else {
                vec![]
            },
            (0..3)
                .map(|channel| Control::Grade {
                    region: region as u32,
                    channel,
                })
                .collect(),
        ));
    }
    layout.extend([
        group(
            "Color",
            "Color Grading · Blending",
            vec![],
            params(&[P::GradingBlending, P::GradingBalance]),
        ),
        group(
            "Color",
            "Calibration",
            vec![panel_switch(Panel::Calibration)],
            params(&[
                P::ShadowTint,
                P::RedPrimaryHue,
                P::RedPrimarySaturation,
                P::GreenPrimaryHue,
                P::GreenPrimarySaturation,
                P::BluePrimaryHue,
                P::BluePrimarySaturation,
            ]),
        ),
        group(
            "Effects",
            "Effects",
            vec![],
            params(&[P::Texture, P::Clarity, P::Dehaze]),
        ),
        group(
            "Effects",
            "Vignette and Grain",
            vec![panel_switch(Panel::Effects)],
            params(&[
                P::VignetteAmount,
                P::VignetteMidpoint,
                P::VignetteFeather,
                P::GrainAmount,
                P::GrainSize,
                P::GrainRoughness,
            ]),
        ),
        group(
            "Detail",
            "AI Denoise",
            vec![Switch::AiDenoise],
            params(&[P::AiDenoiseAmount]),
        ),
        group(
            "Detail",
            "Detail",
            vec![panel_switch(Panel::Detail)],
            params(&[
                P::SharpeningAmount,
                P::SharpeningRadius,
                P::SharpeningDetail,
                P::SharpeningMasking,
                P::LuminanceNoise,
                P::LuminanceDetail,
                P::LuminanceContrast,
                P::ColorNoise,
                P::ColorNoiseDetail,
                P::ColorNoiseSmoothness,
            ]),
        ),
        group(
            "Optics",
            "Optics",
            vec![
                panel_switch(Panel::LensCorrections),
                Switch::LensProfile,
                Switch::LensChromaticAberration,
            ],
            params(&[
                P::LensDistortion,
                P::LensVignetting,
                P::ManualDistortion,
                P::LensVignetteAmount,
                P::LensVignetteMidpoint,
            ]),
        ),
        group("Geometry", "Straighten", vec![], params(&[P::Straighten])),
        group(
            "Geometry",
            "Transform",
            vec![panel_switch(Panel::Transform)],
            params(&[
                P::TransformVertical,
                P::TransformHorizontal,
                P::TransformRotate,
                P::TransformAspect,
                P::TransformScale,
                P::TransformOffsetX,
                P::TransformOffsetY,
            ]),
        ),
    ]);
    layout
        .into_iter()
        .map(|(group, title, toggles, controls)| Section {
            group,
            title,
            switches: toggles.into_iter().filter_map(|t| t.state(r)).collect(),
            sliders: controls
                .into_iter()
                .filter_map(|c| c.state(r, photo, &defaults))
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parameter_is_listed_once() {
        for (i, id) in PARAMS.iter().enumerate() {
            assert!(!PARAMS[..i].contains(id), "{id:?} twice");
        }
        for id in PARAMS {
            assert_eq!(param_at(index_of(id).unwrap()), Some(id));
        }
    }

    #[test]
    fn values_are_shown_as_the_desktop_shows_them() {
        let spec = |c: Control| c.spec(&Metadata::default()).unwrap();
        assert_eq!(
            spec(Control::Hsl {
                band: 0,
                channel: 1
            })
            .text(0.25),
            "+25"
        );
        assert_eq!(
            spec(Control::Hsl {
                band: 0,
                channel: 1
            })
            .text(-0.25),
            "-25"
        );
        assert_eq!(
            spec(Control::Hsl {
                band: 0,
                channel: 1
            })
            .text(0.),
            "0"
        );
        assert_eq!(
            spec(Control::Grade {
                region: 0,
                channel: 0
            })
            .text(0.25),
            "90°"
        );
        assert_eq!(
            spec(Control::Hsl {
                band: 7,
                channel: 2
            })
            .label,
            "Magenta Luminance"
        );
        assert_eq!(
            spec(Control::Grade {
                region: 3,
                channel: 1
            })
            .label,
            "Global Saturation"
        );
    }

    #[test]
    fn mixer_and_grading_values_stay_within_what_the_recipe_accepts() {
        let photo = Metadata::default();
        let mut r = Recipe::default();
        Control::Hsl {
            band: 2,
            channel: 0,
        }
        .set(&mut r, &photo, 3.)
        .unwrap();
        Control::Grade {
            region: 1,
            channel: 2,
        }
        .set(&mut r, &photo, -2.)
        .unwrap();
        Control::Grade {
            region: 3,
            channel: 0,
        }
        .set(&mut r, &photo, 0.5)
        .unwrap();
        Control::Gray { band: 4 }.set(&mut r, &photo, 0.4).unwrap();
        assert_eq!(r.hsl[2][0], 1.);
        assert_eq!(r.grading[1][2], -1.);
        assert_eq!(r.effects.global_grade[0], 0.5);
        assert_eq!(r.effects.gray_mix[4], 0.4);
        r.validate().unwrap();
    }

    #[test]
    fn the_ipad_groups_hold_every_control_of_the_desktops_panels_once() {
        let r = Recipe::default();
        let shown: Vec<Section> = sections(&r, &Metadata::default(), &r);
        let groups: Vec<&str> = shown.iter().map(|s| s.group.as_str()).collect();
        for g in ["Light", "Color", "Effects", "Detail", "Optics", "Geometry"] {
            assert!(groups.contains(&g), "{g}");
        }
        let mut seen: Vec<Control> = Vec::new();
        for s in &shown {
            for c in &s.sliders {
                assert!(!seen.contains(&c.control), "{}/{} twice", s.title, c.label);
                seen.push(c.control);
            }
        }
        let count = |title: &str| {
            shown
                .iter()
                .find(|s| s.title == title)
                .unwrap()
                .sliders
                .len()
        };
        assert_eq!(count("Light"), 6);
        assert_eq!(count("Color"), 4);
        assert_eq!(count("Effects"), 3);
        // Every parameter the panels name is shown somewhere.
        let params: usize = shown
            .iter()
            .flat_map(|s| &s.sliders)
            .filter(|c| matches!(c.control, Control::Param { .. }))
            .count();
        assert!(params >= 50, "{params}");
    }

    #[test]
    fn a_masks_sliders_group_as_lightrooms_masking_panel() {
        let mut r = Recipe::default();
        r.masks.push(Default::default());
        let shown = mask_sections(&r, &Metadata::default(), 0, &Recipe::default());
        let titles: Vec<&str> = shown.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Mask", "Light", "Color", "Effects", "Detail"]);
        let labels = |t: &str| -> Vec<String> {
            shown
                .iter()
                .find(|s| s.title == t)
                .unwrap()
                .sliders
                .iter()
                .map(|s| s.label.clone())
                .collect()
        };
        assert_eq!(labels("Light").len(), 6);
        assert!(labels("Detail").iter().any(|l| l == "Noise"));
        let all: usize = shown.iter().map(|s| s.sliders.len()).sum();
        assert_eq!(
            all,
            mask_sliders(&r, &Metadata::default(), 0, &Recipe::default()).len()
        );
    }
}
