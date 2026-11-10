//! Point Color's range handles: a swatch's three selection ranges and how a
//! handle on one moves.
use rawmakase_model::color::srgb_encode;
use rawmakase_model::model::point_color::PointColor;

/// One of a swatch's three selection ranges, each four points: the outer
/// pair where the selection fades out, the inner pair where it is whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwatchRange {
    /// Across the swatch's hue window, the swatch at 0.5.
    Hue,
    /// On HSV saturation.
    Saturation,
    /// On sRGB-encoded HSV value.
    Luminance,
}

impl SwatchRange {
    pub const ALL: [Self; 3] = [Self::Hue, Self::Saturation, Self::Luminance];

    /// Its label, and the first word of its History step's name ("Point
    /// Color Hue Range").
    pub fn label(self) -> &'static str {
        match self {
            Self::Hue => "Hue Range",
            Self::Saturation => "Sat Range",
            Self::Luminance => "Lum Range",
        }
    }
}

/// Range `which`'s four points.
pub fn range_points(p: &PointColor, which: SwatchRange) -> [f32; 4] {
    match which {
        SwatchRange::Hue => p.hue_range,
        SwatchRange::Saturation => p.saturation_range,
        SwatchRange::Luminance => p.luminance_range,
    }
}

fn range_mut(p: &mut PointColor, which: SwatchRange) -> &mut [f32; 4] {
    match which {
        SwatchRange::Hue => &mut p.hue_range,
        SwatchRange::Saturation => &mut p.saturation_range,
        SwatchRange::Luminance => &mut p.luminance_range,
    }
}

/// Moves range `which`'s point `k` to `t`, between its neighbours, so
/// the swatch stays one Camera Raw accepts: the hue range keeps some
/// span between its inner points, the others keep the sampled value
/// between theirs.
pub fn move_range_point(p: &mut PointColor, which: SwatchRange, k: usize, t: f32) {
    let hold = match which {
        SwatchRange::Hue => None,
        SwatchRange::Saturation => Some(p.source[1]),
        SwatchRange::Luminance => Some(srgb_encode(p.source[2])),
    };
    let Some(points) = (k < 4).then(|| range_mut(p, which)) else {
        return;
    };
    let lo = if k == 0 { 0. } else { points[k - 1] };
    let hi = if k == 3 { 1. } else { points[k + 1] };
    let mut t = t.clamp(lo, hi);
    match (hold, k) {
        (None, 1) => t = t.min(points[2] - 0.01),
        (None, 2) => t = t.max(points[1] + 0.01),
        (Some(v), 1) => t = t.min(v),
        (Some(v), 2) => t = t.max(v),
        _ => {}
    }
    points[k] = t.clamp(0., 1.);
}

/// Range `which` back to a newly sampled swatch's.
pub fn reset_range(p: &mut PointColor, which: SwatchRange) {
    let fresh = range_points(&PointColor::sampled(p.source), which);
    *range_mut(p, which) = fresh;
}

/// The point of `points` a press at `t` takes: the nearest one; among points
/// on one spot, the last when the press is right of them, else the first.
pub fn nearest_point(points: &[f32; 4], t: f32) -> usize {
    let distance = |k: usize| (points[k] - t).abs();
    let best = (0..4).map(distance).fold(f32::INFINITY, f32::min);
    let tied: Vec<usize> = (0..4).filter(|k| distance(*k) - best < 1e-4).collect();
    if t > points[tied[0]] {
        tied[tied.len() - 1]
    } else {
        tied[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_points_keep_their_order_and_the_sampled_value() {
        let mut p = PointColor::sampled([1., 0.55, 0.2]);
        p.saturation_range = [0., 0.4, 0.7, 1.];
        move_range_point(&mut p, SwatchRange::Saturation, 1, 0.9);
        assert_eq!(p.saturation_range, [0., 0.55, 0.7, 1.]);
        move_range_point(&mut p, SwatchRange::Saturation, 2, 0.1);
        assert_eq!(p.saturation_range, [0., 0.55, 0.55, 1.]);
        move_range_point(&mut p, SwatchRange::Saturation, 0, 0.8);
        assert_eq!(p.saturation_range, [0.55, 0.55, 0.55, 1.]);
        move_range_point(&mut p, SwatchRange::Hue, 1, 0.9);
        assert!(p.hue_range[1] < p.hue_range[2]);
        assert!(p.is_valid());
        reset_range(&mut p, SwatchRange::Saturation);
        assert_eq!(
            p.saturation_range,
            PointColor::sampled(p.source).saturation_range
        );
        // Stacked points: a press right of them takes the last, left the first.
        let stacked = [0., 0., 0.5, 1.];
        assert_eq!(nearest_point(&stacked, 0.02), 1);
        assert_eq!(nearest_point(&stacked, 0.), 0);
        assert_eq!(nearest_point(&stacked, 0.7), 2);
    }
}
