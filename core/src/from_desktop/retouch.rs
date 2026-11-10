//! Spot Removal's brushed spots and Red Eye's corrections, as the tools make
//! them.
use rawmakase_engine::develop::red_eye::{PupilGlow, find_pupil};
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::panels::{Panel, PanelState};
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::model::red_eye::{EyeKind, MAX_RADIUS, RedEyeOp};
use rawmakase_model::model::retouch::{MAX_POINTS, RetouchOp};

/// The source a spot gets when no automatic one is found: three radii to the
/// right.
pub fn fallback_offset(op: &RetouchOp) -> [f32; 2] {
    [op.radius() * 3., 0.]
}

/// Adds image-space position `at` to a brushed spot's dabs when it lies a quarter of
/// the brush's `radius` (a long-edge fraction) or more from the last one, on a photo
/// whose width is `aspect` times its height, and the path has room. Whether it did.
pub fn add_dab(points: &mut Vec<[f32; 2]>, at: [f32; 2], radius: f32, aspect: f32) -> bool {
    let Some(last) = points.last() else {
        points.push(at);
        return true;
    };
    // Spacing in long-edge units, whatever the photo's aspect.
    let (dx, dy) = if aspect >= 1. {
        (at[0] - last[0], (at[1] - last[1]) / aspect)
    } else {
        ((at[0] - last[0]) * aspect, at[1] - last[1])
    };
    let spaced = dx.hypot(dy) >= radius * 0.25 && points.len() < MAX_POINTS;
    if spaced {
        points.push(at);
    }
    spaced
}

/// Adds a Red Eye or Pet Eye correction and turns the Red Eye switch on, so it
/// shows, as Lightroom does.
pub fn add_red_eye(r: &mut Recipe, op: RedEyeOp) {
    r.red_eye.push(op);
    r.panels.set(Panel::RedEye, PanelState::On);
}

/// A correction of `kind` for the pupil found within `size` (a fraction of the long
/// edge, at most [`MAX_RADIUS`]) of image-space position `center` in `im`, with Pupil
/// Size and Darken; a Pet Eye's catchlight is pulled within the pupil. `None`
/// when no pupil is found there (RAWmakase doesn't export why).
pub fn correction(
    im: &CameraImage,
    center: [f32; 2],
    size: f32,
    kind: EyeKind,
    pupil_size: f32,
    darken: f32,
) -> Option<RedEyeOp> {
    let pupil = find_pupil(im, center, size.min(MAX_RADIUS), kind.glow()).ok()?;
    let mut op = RedEyeOp {
        kind,
        center: pupil.center,
        radius: pupil.radius,
        correlation: pupil.correlation,
        pupil_size,
        darken,
    };
    op.fit_catchlight();
    Some(op)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dabs_are_a_quarter_radius_apart_on_any_aspect() {
        let mut points = vec![];
        assert!(add_dab(&mut points, [0.5, 0.5], 0.04, 1.5));
        assert!(!add_dab(&mut points, [0.505, 0.5], 0.04, 1.5), "too close");
        assert!(add_dab(&mut points, [0.511, 0.5], 0.04, 1.5));
        // Across a landscape photo's height a step covers more of the long edge.
        assert!(!add_dab(&mut points, [0.511, 0.51], 0.04, 1.5));
        assert!(add_dab(&mut points, [0.511, 0.516], 0.04, 1.5));
        assert_eq!(points.len(), 3);
        let mut full = vec![[0., 0.]; MAX_POINTS];
        assert!(!add_dab(&mut full, [1., 1.], 0.04, 1.), "no room");
    }
}
