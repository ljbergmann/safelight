//! Point Color (Color Mixer › Point Color): up to eight swatches sampled
//! from the photo, each with Hue, Saturation and Luminance shifts, Variance
//! and Range. Sampling and its refusals (too neutral, too dark, already
//! sampled, full) are the engine's, as the desktop's dropper uses them; a
//! swatch's sliders are [`Control::PointColor`].

use crate::controls::{Control, SliderState};
use rawmakase_model::camera_data::Metadata;
use rawmakase_model::model::point_color::{PointColor, SwatchRange};
use rawmakase_model::model::recipe::Recipe;

/// A swatch as the Point Color panel shows it.
#[derive(uniffi::Record, Clone)]
pub struct SwatchInfo {
    pub index: u32,
    /// The sampled color, as sRGB 0–1, for its swatch.
    pub color: Vec<f32>,
    /// What the swatch turns that color into, as sRGB 0–1.
    pub adjusted: Vec<f32>,
    /// Hue, Sat and Lum Shift, Variance and Range.
    pub sliders: Vec<SliderState>,
    /// Its Hue, Sat and Lum ranges.
    pub ranges: Vec<RangeInfo>,
}

/// One of a swatch's selection ranges, as a rail with four handles.
#[derive(uniffi::Record, Clone, Debug)]
pub struct RangeInfo {
    pub label: String,
    /// The outer and inner points, 0–1 along the rail.
    pub points: Vec<f32>,
    /// The rail's colours, evenly spaced from one end to the other, as sRGB
    /// 0–1 triples one after another.
    pub rail: Vec<f32>,
}

/// The ranges, in the order the panel shows them.
pub(crate) const RANGES: [SwatchRange; 3] = SwatchRange::ALL;

/// Stops along a range's rail.
const RAIL_STOPS: usize = 13;

/// Range `which`'s rail: around the swatch's hue for its hue range, its
/// colour from grey to full for saturation, and dark to light for
/// luminance, as the desktop draws them.
fn rail(p: &PointColor, which: SwatchRange) -> Vec<f32> {
    use rawmakase_model::color::hsv::hsv_to_rgb;
    let [h, s, v] = p.source;
    let hue = h / 6. * std::f32::consts::TAU;
    let window = rawmakase_engine::develop::point_color::HUE_WINDOW;
    (0..RAIL_STOPS)
        .flat_map(|i| {
            let t = i as f32 / (RAIL_STOPS - 1) as f32;
            let c = match which {
                SwatchRange::Hue => {
                    hsv_to_rgb(hue + (t - 0.5) * 2. * window, s.max(0.5), v.max(0.5))
                }
                SwatchRange::Saturation => hsv_to_rgb(hue, t, v.max(0.5)),
                SwatchRange::Luminance => {
                    hsv_to_rgb(hue, s, rawmakase_model::color::srgb_decode(t))
                }
            };
            display(c)
        })
        .collect()
}

/// Linear ProPhoto RGB as sRGB for the screen, 0–1.
fn display(p: [f32; 3]) -> Vec<f32> {
    rawmakase_engine::develop::mul(rawmakase_model::camera_profiles::PRO_TO_RGB, p)
        .map(|v| rawmakase_engine::develop::srgb_encode(v.clamp(0., 1.)))
        .to_vec()
}

/// What the swatch turns its sampled color into, through the engine's own
/// Point Color operator, as the desktop's comparison shows it.
fn adjusted(p: &PointColor) -> Vec<f32> {
    let source = p.source_prophoto();
    display(
        rawmakase_engine::develop::point_color::PointColors::new(std::slice::from_ref(p))
            .map_or(source, |op| op.apply_prophoto(source)),
    )
}

/// The swatches of `r`, with their sliders.
pub(crate) fn swatches(r: &Recipe, photo: &Metadata) -> Vec<SwatchInfo> {
    r.point_colors
        .iter()
        .enumerate()
        .map(|(i, p)| SwatchInfo {
            index: i as u32,
            color: display(p.source_prophoto()),
            adjusted: adjusted(p),
            sliders: (0..5)
                .filter_map(|which| {
                    crate::controls::slider(
                        Control::PointColor {
                            swatch: i as u32,
                            which,
                        },
                        r,
                        photo,
                    )
                })
                .collect(),
            ranges: RANGES
                .iter()
                .map(|&which| RangeInfo {
                    label: which.label().into(),
                    points: p.range_points(which).to_vec(),
                    rail: rail(p, which),
                })
                .collect(),
        })
        .collect()
}

/// Adds a swatch of `source` (the engine's sample: HSV of linear ProPhoto
/// RGB, hue in sixths) to `r`'s, as the desktop's dropper does. Returns its
/// index, or why it was refused, worded as the desktop words it.
pub(crate) fn add(r: &mut Recipe, source: [f32; 3]) -> Result<usize, String> {
    let mut list = r.point_colors.clone();
    let index = rawmakase_engine::develop::point_color::add_sample(&mut list, source)
        .map_err(|refusal| refusal.message().replace("click", "sample"))?;
    r.point_colors = list;
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_becomes_a_swatch_with_its_sliders_and_refusals_say_why() {
        let mut r = Recipe::default();
        let photo = Metadata::default();
        // An orange, then the same orange again, then a grey.
        assert_eq!(add(&mut r, [0.5, 0.6, 0.4]), Ok(0));
        let again = add(&mut r, [0.5, 0.6, 0.4]).unwrap_err();
        assert!(again.contains("already"), "{again}");
        let grey = add(&mut r, [0.5, 0.01, 0.4]).unwrap_err();
        assert!(grey.contains("neutral"), "{grey}");

        let list = swatches(&r, &photo);
        assert_eq!(list.len(), 1);
        let labels: Vec<&str> = list[0].sliders.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Hue Shift", "Sat Shift", "Lum Shift", "Variance", "Range"]
        );
        let range = &list[0].sliders[4];
        assert_eq!(
            (range.value, range.reset, range.text.as_str()),
            (0.5, 0.5, "50")
        );

        // A shift moves the swatch's own setting and nothing else.
        let hue = Control::PointColor {
            swatch: 0,
            which: 0,
        };
        hue.set(&mut r, &photo, 0.4).unwrap();
        assert_eq!(r.point_colors[0].shift, [0.4, 0., 0.]);
        assert_ne!(swatches(&r, &photo)[0].adjusted, list[0].adjusted);
        r.validate().unwrap();
    }
}
