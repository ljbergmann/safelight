//! The Straighten ruler and the parametric curve's split handles.
use rawmakase_model::model::effects::Effects;

/// Shortest Straighten ruler, in points, that sets an angle: a click or a slip of the
/// pointer is not a line.
pub const MIN_RULER: f32 = 10.;

/// The Straighten angle that makes the ruler drawn from `from` to `to` on the photo as
/// shown level or, when it is nearer upright, plumb; `current` is the angle the photo is
/// shown at. None for a ruler too short to go by. Positions are in screen points, y down.
pub fn ruler_angle(from: [f32; 2], to: [f32; 2], current: f32) -> Option<f32> {
    let d = [to[0] - from[0], to[1] - from[1]];
    if d[0].hypot(d[1]) < MIN_RULER {
        return None;
    }
    // The ruler's angle, clockwise from level, folded to within a quarter turn of it.
    let angle = d[1].atan2(d[0]).to_degrees();
    let off = (angle + 45.).rem_euclid(90.) - 45.;
    // Straighten turns the photo clockwise, so turning it back by `off` levels the line.
    Some((current - off).clamp(-45., 45.))
}

/// Where the parametric curve's region splits start, as in Lightroom.
pub const DEFAULT_SPLITS: [f32; 3] = [0.25, 0.5, 0.75];

/// Moves split `i` (0 to 2) to `t`, at least 0.05 from its neighbours
/// and the ends, as Lightroom's handles keep them.
pub fn move_split(e: &mut Effects, i: usize, t: f32) {
    if i > 2 {
        return;
    }
    let lo = if i == 0 { 0.05 } else { e.splits[i - 1] + 0.05 };
    let hi = if i == 2 { 0.95 } else { e.splits[i + 1] - 0.05 };
    e.splits[i] = t.clamp(lo, hi);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_split_stays_between_its_neighbours_and_the_ends() {
        let mut e = Effects::default();
        assert_eq!(e.splits, DEFAULT_SPLITS);
        move_split(&mut e, 1, 0.9);
        assert_eq!(e.splits[1], 0.7, "short of the next split");
        move_split(&mut e, 0, 0.);
        assert_eq!(e.splits[0], 0.05);
        move_split(&mut e, 2, 1.);
        assert_eq!(e.splits[2], 0.95);
        move_split(&mut e, 3, 0.5);
        assert_eq!(e.splits, [0.05, 0.7, 0.95], "no fourth split");
    }
}
