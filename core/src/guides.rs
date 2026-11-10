//! The Crop tool's guide overlays, as the desktop's: their names, how many
//! ways each turns, and their lines over a crop, from
//! [`crate::from_desktop::crop_guides`].

use crate::from_desktop::crop_guides::Guide;

/// A guide overlay the Crop tool offers.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct CropGuideInfo {
    /// What [`crop_guide_lines`] takes.
    pub index: u32,
    pub name: String,
    /// How many ways round it turns (the triangle 2, the spiral 4).
    pub orientations: u32,
}

/// The overlays, in Lightroom's order.
#[uniffi::export]
pub fn crop_guides() -> Vec<CropGuideInfo> {
    Guide::ALL
        .iter()
        .enumerate()
        .map(|(i, g)| CropGuideInfo {
            index: i as u32,
            name: g.name().into(),
            orientations: g.orientations().into(),
        })
        .collect()
}

/// Overlay `index`'s lines over a crop `width` × `height` across, from its
/// top-left corner, turned `orientation` ways: each line a polyline of x, y
/// pairs one after another. None for no such overlay.
#[uniffi::export]
pub fn crop_guide_lines(index: u32, orientation: u32, width: f32, height: f32) -> Vec<Vec<f32>> {
    let Some(guide) = Guide::ALL.get(index as usize) else {
        return vec![];
    };
    let orientation = (orientation % u32::from(guide.orientations())) as u8;
    guide
        .lines(orientation, width, height)
        .into_iter()
        .map(|line| line.into_iter().flatten().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirds_cut_the_crop_in_three_each_way_and_the_spiral_turns() {
        let guides = crop_guides();
        let names: Vec<&str> = guides.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Grid",
                "Thirds",
                "Diagonal",
                "Triangle",
                "Golden Ratio",
                "Golden Spiral"
            ]
        );
        let thirds = crop_guide_lines(1, 0, 300., 150.);
        assert!(thirds.contains(&vec![100., 0., 100., 150.]), "{thirds:?}");
        assert!(thirds.contains(&vec![0., 50., 300., 50.]), "{thirds:?}");
        // Turned, the spiral closes on another corner; past its last way round,
        // the turns wrap.
        let spiral = |o| crop_guide_lines(5, o, 300., 150.);
        assert_ne!(spiral(0), spiral(1));
        assert_eq!(spiral(4), spiral(0));
        assert!(crop_guide_lines(9, 0, 300., 150.).is_empty());
    }
}
