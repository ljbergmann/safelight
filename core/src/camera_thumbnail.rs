//! The camera thumbnail: the JPEG the camera embedded in the RAW, shown in
//! place of the photo and labelled as such, as the desktop's view of it
//! (`src/app/develop/camera_thumbnail.rs`, whose size bound and labels this
//! mirrors; the reading is RAWmakase's [`preview_srgb`]). It shows what Fit
//! Camera Look and the Library's first preview start from: unedited and
//! uncropped.

use crate::render::RenderError;
use rawmakase_decode::camera_look::preview_srgb;
use std::path::Path;

/// The longest edge kept: some cameras embed a 60 MP JPEG.
const MAX_EDGE: u32 = 4096;

#[derive(uniffi::Record)]
pub struct CameraThumbnail {
    /// sRGB JPEG.
    pub jpeg: Vec<u8>,
    /// The label over it, as the desktop's.
    pub label: String,
}

/// Reads the camera thumbnail of the RAW at `path`.
pub(crate) fn read(path: &Path) -> Result<CameraThumbnail, RenderError> {
    let preview = preview_srgb(path).map_err(|e| RenderError::Failed {
        message: format!("No camera thumbnail: {e:#}"),
    })?;
    let image = bounded(preview.image);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&image)
        .map_err(|e| RenderError::Failed {
            message: e.to_string(),
        })?;
    let label = match preview.look {
        Some(look) => format!("Camera thumbnail: {look}, embedded JPEG, no edits"),
        None => "Camera thumbnail: embedded JPEG, no edits".to_string(),
    };
    Ok(CameraThumbnail { jpeg, label })
}

/// `im` at most [`MAX_EDGE`] long.
fn bounded(im: image::RgbImage) -> image::RgbImage {
    let edge = im.width().max(im.height());
    if edge <= MAX_EDGE {
        return im;
    }
    let k = MAX_EDGE as f32 / edge as f32;
    image::imageops::resize(
        &im,
        (im.width() as f32 * k) as u32,
        (im.height() as f32 * k) as u32,
        image::imageops::FilterType::Triangle,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A RAW without an embedded JPEG says so; a large thumbnail is bounded.
    #[test]
    fn a_raw_without_a_thumbnail_says_so_and_large_ones_are_bounded() {
        let chart = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        match read(&chart) {
            Ok(t) => {
                let shown = image::load_from_memory(&t.jpeg).unwrap();
                assert!(shown.width().max(shown.height()) <= MAX_EDGE);
                assert!(t.label.starts_with("Camera thumbnail"));
            }
            Err(RenderError::Failed { message }) => {
                assert!(message.starts_with("No camera thumbnail"), "{message}")
            }
            Err(other) => panic!("{other}"),
        }
        let big = bounded(image::RgbImage::new(6000, 4000));
        assert_eq!((big.width(), big.height()), (4096, 2730));
    }
}
