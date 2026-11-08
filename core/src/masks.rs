//! Masks and Heal/Clone: what the iPad's mask and retouch tools change, in
//! the recipe's own terms.
//!
//! Positions arrive as points of the rendered view (0–1 across the photo as
//! shown, cropped) and are turned into image space with the engine's
//! [`ViewMapping`], where RAWmakase keeps masks and spots so they stay on
//! the photo whatever crop, straightening or lens correction do. Sizes are
//! fractions of the long edge, as the model keeps them. Heal and Clone find
//! their source with the engine's [`find_source`], as the desktop's tool does.

use rawmakase_engine::develop::ViewMapping;
use rawmakase_engine::develop::retouch::find_source;
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::masks::{BrushStroke, MaskComponent, MaskGroup, MaskShape};
use rawmakase_model::model::params::LocalParameterId;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::retouch::{RetouchMode, RetouchOp, RetouchShape};
use std::sync::Arc;

/// A mask the Masks panel lists.
#[derive(uniffi::Record)]
pub struct MaskInfo {
    pub index: u32,
    pub name: String,
    /// "Brush", "Linear Gradient", "Radial Gradient" or another kind.
    pub kind: String,
    pub hidden: bool,
}

/// What a new mask starts as.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewMask {
    Brush,
    /// From the first point (full effect) to the second (none).
    Linear,
    /// Centred on the first point, reaching the second.
    Radial,
}

/// A brush dab path from the screen: points of the view (0–1), the brush
/// radius as a fraction of the view's long edge, and how it paints.
#[derive(uniffi::Record, Clone)]
pub struct Stroke {
    /// x, y, x, y… across the photo as shown.
    pub points: Vec<f32>,
    pub radius: f32,
    pub feather: f32,
    /// How much each dab adds, 0–1 (a Pencil's pressure, say).
    pub flow: f32,
    pub erase: bool,
}

/// The mask sliders, in Lightroom's order.
pub const LOCAL: [LocalParameterId; 17] = [
    LocalParameterId::Temperature,
    LocalParameterId::Tint,
    LocalParameterId::Exposure,
    LocalParameterId::Contrast,
    LocalParameterId::Highlights,
    LocalParameterId::Shadows,
    LocalParameterId::Whites,
    LocalParameterId::Blacks,
    LocalParameterId::Texture,
    LocalParameterId::Clarity,
    LocalParameterId::Dehaze,
    LocalParameterId::Hue,
    LocalParameterId::Saturation,
    LocalParameterId::Sharpness,
    LocalParameterId::Noise,
    LocalParameterId::ColorHue,
    LocalParameterId::ColorSaturation,
];

pub(crate) fn infos(r: &Recipe) -> Vec<MaskInfo> {
    r.masks
        .iter()
        .enumerate()
        .map(|(i, m)| MaskInfo {
            index: i as u32,
            name: if m.name.is_empty() {
                format!("Mask {}", i + 1)
            } else {
                m.name.clone()
            },
            kind: m
                .components
                .first()
                .map_or("Empty", |c| match c.shape {
                    MaskShape::Brush { .. } => "Brush",
                    MaskShape::Linear { .. } => "Linear Gradient",
                    MaskShape::Radial { .. } => "Radial Gradient",
                    MaskShape::ColorRange { .. } => "Color Range",
                    MaskShape::LuminanceRange { .. } => "Luminance Range",
                    MaskShape::Bitmap(_) => "Selection",
                })
                .to_string(),
            hidden: m.hidden,
        })
        .collect()
}

/// Maps the view's points into image space, and sizes into fractions of the
/// long edge.
struct Mapper<'a> {
    view: ViewMapping<'a>,
    /// Image space's width and height over its long edge.
    aspect: [f32; 2],
}

impl<'a> Mapper<'a> {
    fn new(image: &'a CameraImage, r: &Recipe) -> Self {
        let view = ViewMapping::new(image, r);
        let (w, h) = (image.width as f32, image.height as f32);
        let long = w.max(h).max(1.);
        Self {
            view,
            aspect: [w / long, h / long],
        }
    }

    fn point(&self, x: f32, y: f32) -> [f32; 2] {
        self.view.to_image(x, y)
    }

    /// A length across the view (fraction of its long edge, from `at`) as a
    /// fraction of the image's long edge.
    fn length(&self, at: [f32; 2], view_length: f32, view_aspect: [f32; 2]) -> f32 {
        let a = self.point(at[0], at[1]);
        let b = self.point(at[0] + view_length / view_aspect[0].max(1e-3), at[1]);
        ((b[0] - a[0]) * self.aspect[0]).hypot((b[1] - a[1]) * self.aspect[1])
    }
}

/// A new mask from two view points (a gradient's ends, or nothing for a brush).
pub(crate) fn new_mask(
    image: &CameraImage,
    r: &mut Recipe,
    kind: NewMask,
    from: [f32; 2],
    to: [f32; 2],
) -> u32 {
    let m = Mapper::new(image, r);
    let shape = match kind {
        NewMask::Brush => MaskShape::Brush { strokes: vec![] },
        NewMask::Linear => MaskShape::Linear {
            from: m.point(from[0], from[1]),
            to: m.point(to[0], to[1]),
        },
        NewMask::Radial => {
            let c = m.point(from[0], from[1]);
            let e = m.point(to[0], to[1]);
            let rx = ((e[0] - c[0]) * m.aspect[0]).abs().max(0.01);
            let ry = ((e[1] - c[1]) * m.aspect[1]).abs().max(0.01);
            MaskShape::Radial {
                center: c,
                radii: [rx, ry],
                angle: 0.,
                feather: 0.5,
            }
        }
    };
    r.masks.push(MaskGroup {
        components: vec![MaskComponent::new(shape)],
        ..MaskGroup::default()
    });
    (r.masks.len() - 1) as u32
}

/// Adds a brush stroke to mask `index` (a brush mask).
pub(crate) fn add_stroke(
    image: &CameraImage,
    r: &mut Recipe,
    index: usize,
    stroke: &Stroke,
    view_aspect: [f32; 2],
) -> bool {
    let points: Vec<[f32; 2]> = stroke.points.as_chunks::<2>().0.to_vec();
    let Some(first) = points.first().copied() else {
        return false;
    };
    let m = Mapper::new(image, r);
    let radius = m.length(first, stroke.radius, view_aspect).max(0.001);
    let mapped: Arc<[[f32; 2]]> = points.iter().map(|p| m.point(p[0], p[1])).collect();
    let Some(MaskShape::Brush { strokes }) = r
        .masks
        .get_mut(index)
        .and_then(|g| g.components.first_mut())
        .map(|c| &mut c.shape)
    else {
        return false;
    };
    strokes.push(BrushStroke {
        points: mapped,
        radius,
        feather: stroke.feather.clamp(0., 1.),
        flow: stroke.flow.clamp(0.05, 1.),
        density: 1.,
        erase: stroke.erase,
        auto_mask: false,
    });
    true
}

/// A Heal or Clone spot at a view point, with a source found as the desktop
/// finds one.
pub(crate) fn add_spot(
    image: &CameraImage,
    r: &mut Recipe,
    at: [f32; 2],
    view_radius: f32,
    view_aspect: [f32; 2],
    clone: bool,
) -> bool {
    let m = Mapper::new(image, r);
    let center = m.point(at[0], at[1]);
    let radius = m.length(at, view_radius, view_aspect).max(0.002);
    let mut op = RetouchOp {
        mode: if clone {
            RetouchMode::Clone
        } else {
            RetouchMode::Heal
        },
        shape: RetouchShape::Spot { center, radius },
        feather: 0.5,
        opacity: 1.,
        offset: [0., 0.],
    };
    let Some(offset) = find_source(image, &op, &r.retouch, &[]) else {
        return false;
    };
    op.offset = offset;
    if op.validate().is_err() {
        return false;
    }
    r.retouch.push(op);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawmakase_cancel::Cancel;
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
    fn new_masks_of_each_kind_are_valid_and_listed() {
        let image = chart();
        let mut r = Recipe::default();
        assert_eq!(
            new_mask(&image, &mut r, NewMask::Linear, [0.5, 0.2], [0.5, 0.8]),
            0
        );
        assert_eq!(
            new_mask(&image, &mut r, NewMask::Radial, [0.5, 0.5], [0.7, 0.6]),
            1
        );
        assert_eq!(
            new_mask(&image, &mut r, NewMask::Brush, [0., 0.], [0., 0.]),
            2
        );
        r.validate().unwrap();
        let kinds: Vec<_> = infos(&r).into_iter().map(|m| m.kind).collect();
        assert_eq!(kinds, ["Linear Gradient", "Radial Gradient", "Brush"]);
        // With no crop or geometry, the view's middle is the image's.
        let MaskShape::Radial { center, .. } = r.masks[1].components[0].shape else {
            panic!()
        };
        assert!(
            (center[0] - 0.5).abs() < 0.01 && (center[1] - 0.5).abs() < 0.01,
            "{center:?}"
        );
    }

    #[test]
    fn a_stroke_lands_in_its_brush_mask() {
        let image = chart();
        let mut r = Recipe::default();
        let i = new_mask(&image, &mut r, NewMask::Brush, [0., 0.], [0., 0.]) as usize;
        let stroke = Stroke {
            points: vec![0.2, 0.2, 0.3, 0.25, 0.4, 0.3],
            radius: 0.05,
            feather: 0.5,
            flow: 0.7,
            erase: false,
        };
        assert!(add_stroke(&image, &mut r, i, &stroke, [1., 0.75]));
        let MaskShape::Brush { strokes } = &r.masks[i].components[0].shape else {
            panic!()
        };
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].points.len(), 3);
        assert!(
            (strokes[0].radius - 0.05).abs() < 0.01,
            "{}",
            strokes[0].radius
        );
        r.validate().unwrap();
    }

    #[test]
    fn a_mask_slider_moves_only_its_mask() {
        let image = chart();
        let mut r = Recipe::default();
        new_mask(&image, &mut r, NewMask::Linear, [0.5, 0.2], [0.5, 0.8]);
        new_mask(&image, &mut r, NewMask::Linear, [0.5, 0.8], [0.5, 0.2]);
        let exposure = LOCAL
            .iter()
            .position(|p| *p == LocalParameterId::Exposure)
            .unwrap() as u32;
        let photo = rawmakase_model::camera_data::Metadata::default();
        crate::controls::Control::Mask {
            mask: 1,
            index: exposure,
        }
        .set(&mut r, &photo, 0.8)
        .unwrap();
        assert_eq!(
            (r.masks[0].adjust.exposure, r.masks[1].adjust.exposure),
            (0., 0.8)
        );
    }

    #[test]
    fn a_heal_spot_gets_a_source_and_stays_valid() {
        let image = chart();
        let mut r = Recipe::default();
        assert!(add_spot(
            &image,
            &mut r,
            [0.4, 0.4],
            0.02,
            [1., 0.75],
            false
        ));
        assert_eq!(r.retouch.len(), 1);
        assert_ne!(r.retouch[0].offset, [0., 0.], "a source was found");
        r.validate().unwrap();
    }
}
