//! Masks: duplicating one, and where a selection's pixels lie.
use rawmakase_model::model::masks::{MAX_GROUPS, MaskGroup};
use rawmakase_model::storage::mask_assets::resolve;

/// Duplicates mask `index` as the next mask, as Lightroom's Duplicate
/// does: a named mask's copy is named "… copy". Returns the copy's index;
/// `None` when there is no such mask or no room for another.
pub fn duplicate(groups: &mut Vec<MaskGroup>, index: usize) -> Option<usize> {
    if groups.len() >= MAX_GROUPS {
        return None;
    }
    let mut copy = groups.get(index)?.clone();
    if !copy.name.is_empty() {
        copy.name.push_str(" copy");
    }
    groups.insert(index + 1, copy);
    Some(index + 1)
}

/// The bounding box, as fractions of the photo `[left, top, right, bottom]`,
/// of raster `id`'s selected pixels (half or more); `None` for an empty or
/// unknown one. What a refinement that only leaves things out works within.
pub fn raster_extent(id: &str) -> Option<[f32; 4]> {
    let raster = resolve(id).ok()?;
    let (w, h) = (raster.width as usize, raster.height as usize);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (i, v) in raster.data.iter().enumerate() {
        if *v >= 128 {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            x1 = x1.max(x + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    (x1 > x0 && y1 > y0).then(|| {
        [
            x0 as f32 / w as f32,
            y0 as f32 / h as f32,
            x1 as f32 / w as f32,
            y1 as f32 / h as f32,
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_follows_its_mask_and_a_named_one_says_copy() {
        let named = MaskGroup {
            name: "Sky".into(),
            ..MaskGroup::default()
        };
        let mut groups = vec![MaskGroup::default(), named];
        assert_eq!(duplicate(&mut groups, 1), Some(2));
        assert_eq!(groups[2].name, "Sky copy");
        assert_eq!(duplicate(&mut groups, 0), Some(1));
        assert_eq!(groups[1].name, "", "an unnamed mask's copy stays unnamed");
        assert_eq!(duplicate(&mut groups, 9), None);
        groups.resize(MAX_GROUPS, MaskGroup::default());
        assert_eq!(duplicate(&mut groups, 0), None, "no room");
    }
}
