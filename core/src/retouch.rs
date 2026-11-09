//! Heal, Clone and Red Eye: what the iPad's Heal tool changes, in the
//! recipe's own terms, with the desktop's rules from the model and engine.
//!
//! Positions arrive as points of the rendered view (0–1 across the photo as
//! shown, cropped) and go into image space through the engine's
//! [`ViewMapping`], where spots and corrections stay put whatever the crop.
//! Sizes arriving from the view are fractions of the view's long edge;
//! the model keeps them as fractions of the image's long edge.

use rawmakase_engine::develop::ViewMapping;
use rawmakase_engine::develop::red_eye;
use rawmakase_engine::develop::retouch::find_source;
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::red_eye::{
    DEFAULT_DARKEN, DEFAULT_PUPIL_SIZE, EyeKind, MAX_OPS as MAX_EYES,
};
use rawmakase_model::model::retouch::{
    MAX_OPS as MAX_SPOTS, RetouchMode, RetouchOp, RetouchShape, add_dab,
};

/// A Heal or Clone spot, for drawing it over the photo.
#[derive(uniffi::Record)]
pub struct SpotInfo {
    pub index: u32,
    pub clone: bool,
    /// Brushed (a path of dabs) rather than a circle.
    pub brushed: bool,
    /// Where its pin sits (the circle's centre or the path's last dab), as a
    /// view point.
    pub pin: Vec<f32>,
    /// The same point of the source it copies from.
    pub source: Vec<f32>,
    /// The dabs' radius, a fraction of the view's long edge.
    pub radius: f32,
    /// A brushed spot's dabs as view points (x, y, x, y…); empty for a circle.
    pub path: Vec<f32>,
    pub feather: f32,
    pub opacity: f32,
}

/// A Red Eye or Pet Eye correction, for drawing and adjusting it.
#[derive(uniffi::Record)]
pub struct RedEyeInfo {
    pub index: u32,
    pub pet: bool,
    /// The ellipse's centre and outline as view points (x, y, x, y…).
    pub center: Vec<f32>,
    pub outline: Vec<f32>,
    pub pupil_size: f32,
    pub darken: f32,
    /// A Pet Eye's catchlight as a view point; empty without one.
    pub catchlight: Vec<f32>,
}

/// Image-space and view geometry for one recipe.
struct Map<'a> {
    view: ViewMapping<'a>,
    /// The image's width and height over its long edge.
    image: [f32; 2],
    /// The view's width and height over its long edge.
    shown: [f32; 2],
}

impl<'a> Map<'a> {
    fn new(image: &'a CameraImage, r: &Recipe, shown: [f32; 2]) -> Self {
        let (w, h) = (image.width as f32, image.height as f32);
        let long = w.max(h).max(1.);
        Self {
            view: ViewMapping::new(image, r),
            image: [w / long, h / long],
            shown,
        }
    }

    fn to_image(&self, p: [f32; 2]) -> [f32; 2] {
        self.view.to_image(p[0], p[1])
    }

    fn to_view(&self, p: [f32; 2]) -> [f32; 2] {
        self.view.to_view(p)
    }

    /// A long-edge fraction of the view at view point `at`, as one of the image.
    fn size_in_image(&self, at: [f32; 2], view: f32) -> f32 {
        let a = self.to_image(at);
        let b = self.to_image([at[0] + view / self.shown[0].max(1e-3), at[1]]);
        ((b[0] - a[0]) * self.image[0]).hypot((b[1] - a[1]) * self.image[1])
    }

    /// A long-edge fraction of the image at image point `at`, as one of the view.
    fn size_in_view(&self, at: [f32; 2], image: f32) -> f32 {
        let a = self.to_view(at);
        let b = self.to_view([at[0] + image / self.image[0].max(1e-3), at[1]]);
        ((b[0] - a[0]) * self.shown[0]).hypot((b[1] - a[1]) * self.shown[1])
    }

    /// Width over height of the oriented photo, as the model's shapes take it.
    fn aspect(&self) -> f32 {
        self.view.frame().aspect()
    }
}

pub(crate) fn spots(image: &CameraImage, r: &Recipe, shown: [f32; 2]) -> Vec<SpotInfo> {
    let m = Map::new(image, r, shown);
    r.retouch
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let pin = op.pin();
            let source = [pin[0] + op.offset[0], pin[1] + op.offset[1]];
            let path = match &op.shape {
                RetouchShape::Spot { .. } => vec![],
                RetouchShape::Brush { points, .. } => {
                    points.iter().flat_map(|p| m.to_view(*p)).collect()
                }
            };
            SpotInfo {
                index: i as u32,
                clone: op.mode == RetouchMode::Clone,
                brushed: !path.is_empty(),
                pin: m.to_view(pin).to_vec(),
                source: m.to_view(source).to_vec(),
                radius: m.size_in_view(pin, op.radius()),
                path,
                feather: op.feather,
                opacity: op.opacity,
            }
        })
        .collect()
}

/// A Heal or Clone spot: a circle at one view point, or brushed along
/// several (dabs spaced as the desktop spaces them). Its source is found as
/// the desktop finds it. Returns its index.
#[allow(clippy::too_many_arguments)]
pub(crate) fn add_spot(
    image: &CameraImage,
    r: &mut Recipe,
    points: &[[f32; 2]],
    view_radius: f32,
    shown: [f32; 2],
    clone: bool,
    feather: f32,
    opacity: f32,
) -> Option<u32> {
    let first = *points.first()?;
    if r.retouch.len() >= MAX_SPOTS {
        return None;
    }
    let m = Map::new(image, r, shown);
    let radius = m.size_in_image(first, view_radius).clamp(0.001, 0.25);
    let shape = if points.len() == 1 {
        RetouchShape::Spot {
            center: m.to_image(first),
            radius,
        }
    } else {
        let aspect = m.aspect();
        let mut dabs = vec![m.to_image(first)];
        for p in &points[1..] {
            add_dab(&mut dabs, m.to_image(*p), radius, aspect);
        }
        RetouchShape::Brush {
            points: dabs.into(),
            radius,
        }
    };
    let mut op = RetouchOp {
        mode: if clone {
            RetouchMode::Clone
        } else {
            RetouchMode::Heal
        },
        shape,
        feather: feather.clamp(0., 1.),
        opacity: opacity.clamp(0., 1.),
        offset: [0., 0.],
    };
    op.offset = find_source(image, &op, &r.retouch, &[]).unwrap_or_else(|| op.fallback_offset());
    op.validate().ok()?;
    r.add_retouch(op);
    Some(r.retouch.len() as u32 - 1)
}

/// Moves spot `index`, or with `source` where it copies from, by the drag
/// from view point `from` to `to`.
pub(crate) fn move_spot(
    image: &CameraImage,
    r: &mut Recipe,
    index: usize,
    from: [f32; 2],
    to: [f32; 2],
    source: bool,
) -> bool {
    let m = Map::new(image, r, [1., 1.]);
    let (a, b) = (m.to_image(from), m.to_image(to));
    let delta = [b[0] - a[0], b[1] - a[1]];
    let Some(op) = r.retouch.get_mut(index) else {
        return false;
    };
    let mut moved = op.clone();
    if source {
        moved.offset = [moved.offset[0] + delta[0], moved.offset[1] + delta[1]];
    } else {
        moved.translate(delta);
    }
    if moved.validate().is_err() {
        return false;
    }
    *op = moved;
    true
}

/// Spot `index`'s Feather, Opacity and size (a fraction of the view's long
/// edge), and whether it heals or clones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn set_spot(
    image: &CameraImage,
    r: &mut Recipe,
    index: usize,
    shown: [f32; 2],
    view_radius: f32,
    feather: f32,
    opacity: f32,
    clone: bool,
) -> bool {
    let m = Map::new(image, r, shown);
    let Some(op) = r.retouch.get(index) else {
        return false;
    };
    let mut changed = op.clone();
    let pin = m.to_view(changed.pin());
    changed.set_radius(m.size_in_image(pin, view_radius).clamp(0.001, 0.25));
    changed.feather = feather.clamp(0., 1.);
    changed.opacity = opacity.clamp(0., 1.);
    changed.mode = if clone {
        RetouchMode::Clone
    } else {
        RetouchMode::Heal
    };
    if changed.validate().is_err() || changed == r.retouch[index] {
        return false;
    }
    r.retouch[index] = changed;
    true
}

/// The next best source for spot `index`, past those in `tried` (offsets),
/// as the desktop's `/` finds one. Returns the offset it had, for `tried`.
pub(crate) fn next_source(
    image: &CameraImage,
    r: &mut Recipe,
    index: usize,
    tried: &mut Vec<[f32; 2]>,
) -> bool {
    let Some(op) = r.retouch.get(index).cloned() else {
        return false;
    };
    if !tried.contains(&op.offset) {
        tried.push(op.offset);
    }
    let Some(offset) = find_source(image, &op, &r.retouch, tried) else {
        return false;
    };
    r.retouch[index].offset = offset;
    true
}

pub(crate) fn red_eyes(image: &CameraImage, r: &Recipe) -> Vec<RedEyeInfo> {
    let m = Map::new(image, r, [1., 1.]);
    let aspect = m.aspect();
    r.red_eye
        .iter()
        .enumerate()
        .map(|(i, op)| RedEyeInfo {
            index: i as u32,
            pet: matches!(op.kind, EyeKind::Pet { .. }),
            center: m.to_view(op.center).to_vec(),
            outline: op
                .outline(aspect, 48)
                .into_iter()
                .flat_map(|p| m.to_view(p))
                .collect(),
            pupil_size: op.pupil_size,
            darken: op.darken,
            catchlight: op
                .catchlight_at(aspect)
                .map(|p| m.to_view(p).to_vec())
                .unwrap_or_default(),
        })
        .collect()
}

/// Corrects the red (or, for `pet`, glowing) pupil within `view_size` (a
/// fraction of the view's long edge) of view point `at`, with the desktop's
/// search and Lightroom's defaults; Pet Eye gets Lightroom's catchlight.
/// Returns its index, or Lightroom's warning when there is no pupil there.
pub(crate) fn add_red_eye(
    image: &CameraImage,
    r: &mut Recipe,
    at: [f32; 2],
    view_size: f32,
    shown: [f32; 2],
    pet: bool,
) -> Result<u32, String> {
    if r.red_eye.len() >= MAX_EYES {
        return Err("Too many red eye corrections on this photo".into());
    }
    let m = Map::new(image, r, shown);
    let kind = if pet {
        EyeKind::Pet {
            catchlight: Some(rawmakase_model::model::red_eye::DEFAULT_CATCHLIGHT),
        }
    } else {
        EyeKind::Red
    };
    let size = m.size_in_image(at, view_size);
    let op = red_eye::correction(
        image,
        m.to_image(at),
        size,
        kind,
        DEFAULT_PUPIL_SIZE,
        DEFAULT_DARKEN,
    )
    .map_err(|_| {
        format!(
            "Unable to find {}. Be sure to use an area that includes the entire eye.",
            kind.name().to_lowercase()
        )
    })?;
    op.validate().map_err(|e| e.to_string())?;
    r.add_red_eye(op);
    Ok(r.red_eye.len() as u32 - 1)
}

/// Correction `index`'s Pupil Size and Darken.
pub(crate) fn set_red_eye(r: &mut Recipe, index: usize, pupil_size: f32, darken: f32) -> bool {
    let Some(op) = r.red_eye.get_mut(index) else {
        return false;
    };
    let (pupil_size, darken) = (pupil_size.clamp(0., 1.), darken.clamp(0., 1.));
    if (op.pupil_size, op.darken) == (pupil_size, darken) {
        return false;
    }
    op.pupil_size = pupil_size;
    op.darken = darken;
    true
}

/// Gives Pet Eye correction `index` Lightroom's catchlight, or takes it
/// away, as the desktop's Add Catchlight.
pub(crate) fn set_catchlight(r: &mut Recipe, index: usize, on: bool) -> bool {
    let Some(op) = r.red_eye.get_mut(index) else {
        return false;
    };
    let EyeKind::Pet { catchlight } = &mut op.kind else {
        return false;
    };
    if catchlight.is_some() == on {
        return false;
    }
    *catchlight = on.then_some(rawmakase_model::model::red_eye::DEFAULT_CATCHLIGHT);
    op.fit_catchlight();
    true
}

/// Places Pet Eye correction `index`'s catchlight at view point `to`, kept
/// within the pupil.
pub(crate) fn move_catchlight(
    image: &CameraImage,
    r: &mut Recipe,
    index: usize,
    to: [f32; 2],
) -> bool {
    let m = Map::new(image, r, [1., 1.]);
    let (at, aspect) = (m.to_image(to), m.aspect());
    let Some(op) = r.red_eye.get_mut(index) else {
        return false;
    };
    let before = op.kind;
    op.set_catchlight(at, aspect);
    op.kind != before
}

/// Moves correction `index` by the drag from view point `from` to `to`.
pub(crate) fn move_red_eye(
    image: &CameraImage,
    r: &mut Recipe,
    index: usize,
    from: [f32; 2],
    to: [f32; 2],
) -> bool {
    let m = Map::new(image, r, [1., 1.]);
    let (a, b) = (m.to_image(from), m.to_image(to));
    let Some(op) = r.red_eye.get_mut(index) else {
        return false;
    };
    op.translate([b[0] - a[0], b[1] - a[1]]);
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

    /// The chart with a red pupil in a brown iris at its centre, `pupil`
    /// pixels across (camera values, as RAWmakase's own red eye test).
    fn face(pupil: f32) -> CameraImage {
        let mut im = chart();
        let w = im.width as usize;
        let c = [im.width as f32 * 0.5, im.height as f32 * 0.5];
        for (i, p) in im.pixels.iter_mut().enumerate() {
            let d = ((i % w) as f32 - c[0]).hypot((i / w) as f32 - c[1]);
            *p = if d <= pupil {
                [0.5, 0.03, 0.03]
            } else if d <= 2.5 * pupil {
                [0.2, 0.07, 0.04]
            } else {
                [0.5, 0.33, 0.24]
            };
        }
        im
    }

    #[test]
    fn spots_are_placed_brushed_moved_and_changed() {
        let image = chart();
        let mut r = Recipe::default();
        let shown = [1., 0.75];
        let circle = add_spot(&image, &mut r, &[[0.4, 0.4]], 0.02, shown, false, 0.5, 1.).unwrap();
        assert_eq!(circle, 0);
        assert_ne!(r.retouch[0].offset, [0., 0.], "a source was found");
        let line: Vec<[f32; 2]> = (0..40).map(|i| [0.2 + i as f32 * 0.001, 0.6]).collect();
        let brushed = add_spot(&image, &mut r, &line, 0.01, shown, true, 0.3, 0.8).unwrap();
        let RetouchShape::Brush { points, .. } = &r.retouch[1].shape else {
            panic!("a brushed spot")
        };
        assert!(
            points.len() > 2 && points.len() < 40,
            "dabs spaced: {}",
            points.len()
        );
        let listed = spots(&image, &r, shown);
        assert!(listed[1].brushed && listed[1].clone);
        assert!((listed[0].pin[0] - 0.4).abs() < 1e-3, "{:?}", listed[0].pin);
        assert!(
            (listed[0].radius - 0.02).abs() < 2e-3,
            "{}",
            listed[0].radius
        );

        let before = r.retouch[0].clone();
        assert!(move_spot(&image, &mut r, 0, [0.4, 0.4], [0.5, 0.4], false));
        let after = spots(&image, &r, shown);
        assert!((after[0].pin[0] - 0.5).abs() < 1e-3);
        assert!(
            (after[0].source[0] - listed[0].source[0]).abs() < 1e-3,
            "the source stays put"
        );
        assert!(move_spot(&image, &mut r, 0, [0.5, 0.4], [0.5, 0.5], true));
        assert_ne!(r.retouch[0].offset, before.offset);

        assert!(set_spot(
            &image,
            &mut r,
            brushed as usize,
            shown,
            0.03,
            1.,
            0.25,
            false
        ));
        assert_eq!(r.retouch[1].mode, RetouchMode::Heal);
        assert_eq!((r.retouch[1].feather, r.retouch[1].opacity), (1., 0.25));
        assert!(!set_spot(&image, &mut r, 5, shown, 0.03, 1., 0.25, false));

        let mut tried = vec![];
        let first = r.retouch[0].offset;
        if next_source(&image, &mut r, 0, &mut tried) {
            assert_ne!(r.retouch[0].offset, first, "a new source");
        }
        assert_eq!(tried, vec![first]);
        r.validate().unwrap();
    }

    #[test]
    fn a_red_pupil_is_found_adjusted_and_moved() {
        let image = face(6.);
        let mut r = Recipe::default();
        let shown = [1., image.height as f32 / image.width as f32];
        let i = add_red_eye(&image, &mut r, [0.5, 0.5], 0.04, shown, false).unwrap() as usize;
        let listed = red_eyes(&image, &r);
        assert!(!listed[0].pet);
        assert!(
            (listed[0].center[0] - 0.5).abs() < 0.01,
            "{:?}",
            listed[0].center
        );
        assert_eq!(
            (listed[0].pupil_size, listed[0].darken),
            (DEFAULT_PUPIL_SIZE, DEFAULT_DARKEN)
        );
        assert_eq!(
            r.panels
                .state(rawmakase_model::model::panels::Panel::RedEye),
            rawmakase_model::model::panels::PanelState::On,
            "the switch turned on"
        );
        assert!(set_red_eye(&mut r, i, 0.8, 0.2));
        assert!(!set_red_eye(&mut r, i, 0.8, 0.2), "no change");
        assert!(move_red_eye(&image, &mut r, i, [0.5, 0.5], [0.6, 0.5]));
        assert!((red_eyes(&image, &r)[0].center[0] - 0.6).abs() < 0.01);
        r.validate().unwrap();
    }

    /// A Pet Eye's catchlight is listed where it is, can be taken away and
    /// given back (Lightroom's), and dragged, staying within the pupil. A
    /// Red Eye has none.
    #[test]
    fn a_pet_eyes_catchlight_is_switched_and_dragged() {
        use rawmakase_model::model::red_eye::DEFAULT_CATCHLIGHT;
        let image = face(6.);
        let mut r = Recipe::default();
        let shown = [1., image.height as f32 / image.width as f32];
        let i = add_red_eye(&image, &mut r, [0.5, 0.5], 0.04, shown, false).unwrap() as usize;
        assert!(red_eyes(&image, &r)[0].catchlight.is_empty());
        assert!(!set_catchlight(&mut r, i, true), "a red eye has none");
        r.red_eye[i].kind = EyeKind::Pet {
            catchlight: Some(DEFAULT_CATCHLIGHT),
        };
        let at = red_eyes(&image, &r)[0].catchlight.clone();
        assert_eq!(at.len(), 2);
        assert!(
            at[0] > 0.5 && at[1] < 0.5,
            "up and right of the centre: {at:?}"
        );

        assert!(set_catchlight(&mut r, i, false));
        assert!(red_eyes(&image, &r)[0].catchlight.is_empty());
        assert!(!set_catchlight(&mut r, i, false), "already off");
        assert!(set_catchlight(&mut r, i, true));
        assert_eq!(
            r.red_eye[i].kind,
            EyeKind::Pet {
                catchlight: Some(DEFAULT_CATCHLIGHT)
            }
        );

        // Dragged far outside the pupil, it stays at the pupil's edge.
        assert!(move_catchlight(&image, &mut r, i, [0.2, 0.5]));
        let moved = red_eyes(&image, &r)[0].catchlight.clone();
        assert!(moved[0] < 0.5 && moved[0] > 0.45, "{moved:?}");
        r.validate().unwrap();
    }

    #[test]
    fn no_pupil_gives_lightrooms_warning() {
        let image = chart();
        let mut r = Recipe::default();
        let err = add_red_eye(&image, &mut r, [0.5, 0.5], 0.05, [1., 0.75], false).unwrap_err();
        assert!(err.starts_with("Unable to find red eye"), "{err}");
        assert!(r.red_eye.is_empty());
        assert!(!set_red_eye(&mut r, 0, 0.2, 0.2));
    }
}
