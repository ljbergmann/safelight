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
use rawmakase_engine::develop::masks::Space;
use rawmakase_engine::develop::retouch::find_source;
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::masks::{BrushStroke, MaskComponent, MaskGroup, MaskOp, MaskShape};
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
    /// The whole mask inverted.
    pub invert: bool,
    /// Its parts, in the order they combine.
    pub components: Vec<ComponentInfo>,
}

/// What a new mask starts as.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewMask {
    Brush,
    /// From the first point (full effect) to the second (none).
    Linear,
    /// Centred on the first point, reaching the second.
    Radial,
    /// The colour at the first point, as the photo is developed.
    ColorRange,
    /// The brightness around the first point's.
    LuminanceRange,
}

/// How a component combines with the ones before it, as Lightroom's Add,
/// Subtract and Intersect.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskOperation {
    Add,
    Subtract,
    Intersect,
}

impl MaskOperation {
    fn op(self) -> MaskOp {
        match self {
            Self::Add => MaskOp::Add,
            Self::Subtract => MaskOp::Subtract,
            Self::Intersect => MaskOp::Intersect,
        }
    }

    fn of(op: MaskOp) -> Self {
        match op {
            MaskOp::Add => Self::Add,
            MaskOp::Subtract => Self::Subtract,
            MaskOp::Intersect => Self::Intersect,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Intersect => "Intersect",
        }
    }
}

/// One part of a mask, as the Masks panel lists it.
#[derive(uniffi::Record)]
pub struct ComponentInfo {
    pub index: u32,
    pub kind: String,
    pub operation: MaskOperation,
    pub invert: bool,
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
                .map_or("Empty", |c| kind(&c.shape))
                .to_string(),
            hidden: m.hidden,
            invert: m.invert,
            components: m
                .components
                .iter()
                .enumerate()
                .map(|(i, c)| ComponentInfo {
                    index: i as u32,
                    kind: kind(&c.shape).into(),
                    operation: MaskOperation::of(c.op),
                    invert: c.invert,
                })
                .collect(),
        })
        .collect()
}

fn kind(shape: &MaskShape) -> &'static str {
    match shape {
        MaskShape::Brush { .. } => "Brush",
        MaskShape::Linear { .. } => "Linear Gradient",
        MaskShape::Radial { .. } => "Radial Gradient",
        MaskShape::ColorRange { .. } => "Color Range",
        MaskShape::LuminanceRange { .. } => "Luminance Range",
        MaskShape::Bitmap(_) => "Selection",
    }
}

/// The History step's value for a new mask or component of `kind`.
pub(crate) fn new_name(kind: NewMask) -> &'static str {
    match kind {
        NewMask::Brush => "Brush",
        NewMask::Linear => "Linear Gradient",
        NewMask::Radial => "Radial Gradient",
        NewMask::ColorRange => "Color Range",
        NewMask::LuminanceRange => "Luminance Range",
    }
}

/// The developed colour at view point `at`, in Oklab, as the desktop's range
/// masks sample it: the photo without its masks or sharpening, averaged over
/// 3 × 3 pixels. Mirrors `app::develop::tools::mask_tool::developed_lab`.
fn developed_lab(image: &CameraImage, r: &Recipe, at: [f32; 2]) -> Option<[f32; 3]> {
    let mut r = r.clone();
    r.masks.clear();
    r.sharpening = 0.;
    let g = rawmakase_engine::develop::Geometry::new(image, &r, 0);
    let (w, h) = (3.min(g.width), 3.min(g.height));
    let (u, v) = (at[0].clamp(0., 1.), at[1].clamp(0., 1.));
    let x = ((u * g.width as f32) as u32)
        .saturating_sub(1)
        .min(g.width - w);
    let y = ((v * g.height as f32) as u32)
        .saturating_sub(1)
        .min(g.height - h);
    let out =
        rawmakase_engine::develop::render_region(image, &r.checked().ok()?, [x, y, w, h]).ok()?;
    let n = out.pixels.len() as f32;
    let mean = out
        .pixels
        .iter()
        .fold([0.; 3], |s, p| std::array::from_fn(|c| s[c] + p[c] / n));
    Some(rawmakase_engine::develop::masks::oklab(mean))
}

/// A shape of `kind` from two view points; `None` when a range mask's point
/// can't be sampled.
fn shape(
    image: &CameraImage,
    r: &Recipe,
    kind: NewMask,
    from: [f32; 2],
    to: [f32; 2],
) -> Option<MaskShape> {
    let m = Mapper::new(image, r);
    Some(match kind {
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
        // As the desktop starts them.
        NewMask::ColorRange => MaskShape::ColorRange {
            samples: vec![developed_lab(image, r, from)?],
            amount: 0.5,
        },
        NewMask::LuminanceRange => {
            let l = developed_lab(image, r, from)?[0].clamp(0., 1.);
            MaskShape::LuminanceRange {
                low: (l - 0.1).max(0.),
                high: (l + 0.1).min(1.),
                falloff: [0.15, 0.15],
            }
        }
    })
}

/// Adds a part of `kind` to mask `mask`, combined by `operation`; returns its
/// index.
pub(crate) fn add_component(
    image: &CameraImage,
    r: &mut Recipe,
    mask: usize,
    kind: NewMask,
    operation: MaskOperation,
    from: [f32; 2],
    to: [f32; 2],
) -> Option<u32> {
    let shape = shape(image, r, kind, from, to)?;
    let group = r.masks.get_mut(mask)?;
    let mut component = MaskComponent::new(shape);
    component.op = operation.op();
    group.components.push(component);
    Some((group.components.len() - 1) as u32)
}

/// A placed gradient's handles, as view points (x, y, x, y…): a linear
/// gradient's start, end and middle; a radial one's centre and its two radii.
/// Mirrors the desktop's `app::develop::tools::mask_tool::handles`.
pub(crate) fn handles(image: &CameraImage, r: &Recipe, mask: usize, component: usize) -> Vec<f32> {
    let Some(shape) = r
        .masks
        .get(mask)
        .and_then(|m| m.components.get(component))
        .map(|c| &c.shape)
    else {
        return vec![];
    };
    let map = ViewMapping::new(image, r);
    let space = Space::new(map.frame().aspect());
    let view = |p: [f32; 2]| map.to_view(p);
    let points: Vec<[f32; 2]> = match shape {
        MaskShape::Linear { from, to } => vec![
            view(*from),
            view(*to),
            view([(from[0] + to[0]) / 2., (from[1] + to[1]) / 2.]),
        ],
        MaskShape::Radial {
            center,
            radii,
            angle,
            ..
        } => {
            let c = space.to(*center);
            let (s, co) = angle.to_radians().sin_cos();
            let edge = |k: usize| {
                let (x, y) = if k == 0 {
                    (radii[0] * co, radii[0] * s)
                } else {
                    (-radii[1] * s, radii[1] * co)
                };
                view(space.from([c[0] + x, c[1] + y]))
            };
            vec![view(*center), edge(0), edge(1)]
        }
        _ => vec![],
    };
    points.into_iter().flatten().collect()
}

/// Moves handle `handle` (as [`handles`] orders them) of a placed gradient
/// from view point `from` to `to`: an end moves alone, the middle or centre
/// moves the whole gradient, and a radius follows the touch's distance and
/// turns the ellipse. Mirrors the desktop's `mask_tool::move_handle`.
pub(crate) fn move_handle(
    image: &CameraImage,
    r: &mut Recipe,
    mask: usize,
    component: usize,
    handle: u32,
    from: [f32; 2],
    to: [f32; 2],
) -> bool {
    let map = ViewMapping::new(image, r);
    let space = Space::new(map.frame().aspect());
    let (start, at) = (map.to_image(from[0], from[1]), map.to_image(to[0], to[1]));
    let delta = [at[0] - start[0], at[1] - start[1]];
    let shift = |p: [f32; 2]| [p[0] + delta[0], p[1] + delta[1]];
    let Some(shape) = r
        .masks
        .get_mut(mask)
        .and_then(|m| m.components.get_mut(component))
        .map(|c| &mut c.shape)
    else {
        return false;
    };
    match shape {
        MaskShape::Linear { from, to } => match handle {
            0 => *from = shift(*from),
            1 => *to = shift(*to),
            _ => {
                *from = shift(*from);
                *to = shift(*to);
            }
        },
        MaskShape::Radial {
            center,
            radii,
            angle,
            ..
        } => match handle {
            k @ (1 | 2) => {
                let k = k as usize - 1;
                let (c, p) = (space.to(*center), space.to(at));
                let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
                radii[k] = dx.hypot(dy).max(0.005);
                let turn = dy.atan2(dx).to_degrees() - if k == 0 { 0. } else { 90. };
                *angle = (turn + 180.).rem_euclid(360.) - 180.;
            }
            _ => *center = shift(*center),
        },
        _ => return false,
    }
    true
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
) -> Option<u32> {
    let shape = shape(image, r, kind, from, to)?;
    r.masks.push(MaskGroup {
        components: vec![MaskComponent::new(shape)],
        ..MaskGroup::default()
    });
    Some((r.masks.len() - 1) as u32)
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
    // Into the mask's last brush part: the one added most recently.
    let Some(MaskShape::Brush { strokes }) = r
        .masks
        .get_mut(index)
        .and_then(|g| {
            g.components
                .iter_mut()
                .rev()
                .find(|c| matches!(c.shape, MaskShape::Brush { .. }))
        })
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
            Some(0)
        );
        assert_eq!(
            new_mask(&image, &mut r, NewMask::Radial, [0.5, 0.5], [0.7, 0.6]),
            Some(1)
        );
        assert_eq!(
            new_mask(&image, &mut r, NewMask::Brush, [0., 0.], [0., 0.]),
            Some(2)
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
        let i = new_mask(&image, &mut r, NewMask::Brush, [0., 0.], [0., 0.]).unwrap() as usize;
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
    fn range_masks_and_parts_combine_as_in_lightroom() {
        let image = chart();
        let mut r = Recipe::default();
        let color = new_mask(&image, &mut r, NewMask::ColorRange, [0.3, 0.3], [0., 0.]).unwrap();
        let luminance = new_mask(
            &image,
            &mut r,
            NewMask::LuminanceRange,
            [0.6, 0.6],
            [0., 0.],
        )
        .unwrap();
        let part = add_component(
            &image,
            &mut r,
            color as usize,
            NewMask::Radial,
            MaskOperation::Intersect,
            [0.5, 0.5],
            [0.6, 0.6],
        )
        .unwrap();
        r.validate().unwrap();
        let listed = infos(&r);
        assert_eq!(listed[luminance as usize].kind, "Luminance Range");
        let parts = &listed[color as usize].components;
        assert_eq!(parts.len(), 2);
        assert_eq!(
            (
                parts[part as usize].kind.as_str(),
                parts[part as usize].operation
            ),
            ("Radial Gradient", MaskOperation::Intersect)
        );
        let MaskShape::ColorRange { samples, .. } = &r.masks[0].components[0].shape else {
            panic!()
        };
        assert!(samples[0].iter().all(|v| v.is_finite()));
    }

    #[test]
    fn range_and_amount_sliders_keep_the_mask_valid() {
        use crate::controls::{Control, mask_sliders};
        let image = chart();
        let photo = rawmakase_model::camera_data::Metadata::default();
        let mut r = Recipe::default();
        let m = new_mask(
            &image,
            &mut r,
            NewMask::LuminanceRange,
            [0.5, 0.5],
            [0., 0.],
        )
        .unwrap();
        let labels: Vec<_> = mask_sliders(&r, &photo, m, &Recipe::default())
            .into_iter()
            .take(5)
            .map(|s| s.label)
            .collect();
        assert_eq!(
            labels,
            ["Amount", "Low", "High", "Falloff Low", "Falloff High"]
        );
        // Low past High takes High along.
        Control::Range {
            mask: m,
            component: 0,
            which: 0,
        }
        .set(&mut r, &photo, 0.9)
        .unwrap();
        Control::MaskAmount { mask: m }
            .set(&mut r, &photo, 1.5)
            .unwrap();
        let MaskShape::LuminanceRange { low, high, .. } = r.masks[0].components[0].shape else {
            panic!()
        };
        assert_eq!((low, high, r.masks[0].amount), (0.9, 0.9, 1.5));
        r.validate().unwrap();
    }

    #[test]
    fn a_placed_gradient_moves_by_its_handles() {
        let image = chart();
        let mut r = Recipe::default();
        let m = new_mask(&image, &mut r, NewMask::Radial, [0.5, 0.5], [0.6, 0.6]).unwrap() as usize;
        let before = handles(&image, &r, m, 0);
        assert_eq!(before.len(), 6);
        assert!((before[0] - 0.5).abs() < 0.01 && (before[1] - 0.5).abs() < 0.01);
        // The centre moves the ellipse; its handles follow.
        assert!(move_handle(
            &image,
            &mut r,
            m,
            0,
            0,
            [0.5, 0.5],
            [0.4, 0.45]
        ));
        let after = handles(&image, &r, m, 0);
        assert!(
            (after[0] - 0.4).abs() < 0.01 && (after[1] - 0.45).abs() < 0.01,
            "{after:?}"
        );
        // A radius follows the touch.
        assert!(move_handle(
            &image,
            &mut r,
            m,
            0,
            1,
            [after[2], after[3]],
            [0.7, 0.45]
        ));
        let MaskShape::Radial { radii, .. } = r.masks[m].components[0].shape else {
            panic!()
        };
        assert!(radii[0] > 0.2, "{radii:?}");
        r.validate().unwrap();
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
