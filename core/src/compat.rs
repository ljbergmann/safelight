//! Compatibility with RAWmakase on a computer: a library edited through this
//! core must open in the desktop app and render there as it does here. The
//! test drives the desktop's own command line (`RAWMAKASE_BIN`), so it checks
//! the real reader of the catalog, not a copy of it; `scripts/compat-check.sh`
//! builds that binary and runs it.

#[cfg(test)]
mod tests {
    use crate::export::{ExportChoice, ExportFormat};
    use crate::library::Library;
    use rawmakase_cancel::Cancel;
    use std::path::Path;

    /// The image's pixels, 8 bits a channel.
    fn pixels(path: &Path) -> image::RgbImage {
        image::open(path).unwrap().to_rgb8()
    }

    /// A photo edited here (global sliders, a crop, a radial mask and a Heal
    /// spot) renders pixel for pixel the same in the desktop's `rawmakase
    /// render --catalog` as in this core's export, both lossless PNGs at full
    /// size: the desktop reads the catalog and the edit as this core wrote
    /// them. Needs `RAWMAKASE_BIN`, the desktop binary built from
    /// `vendor/rawmakase`, and both sides reading the same profiles (one
    /// `HOME`; the script gives an empty one).
    #[test]
    #[ignore = "needs RAWMAKASE_BIN; run scripts/compat-check.sh"]
    fn an_edit_made_here_renders_the_same_in_the_desktop_app() {
        let bin = std::env::var("RAWMAKASE_BIN").expect("RAWMAKASE_BIN");
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let library = Library::open(
            data.display().to_string(),
            dir.path().join("caches").display().to_string(),
        )
        .unwrap();
        let day = Path::new(&library.photos_folder()).join("d");
        std::fs::create_dir_all(&day).unwrap();
        let chart = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        std::fs::copy(chart, day.join("chart.dng")).unwrap();
        library.add_new_photos().unwrap();
        let photo = library.photos().unwrap().remove(0);
        let path = Path::new(&photo.path);

        // Edited as the app edits: through the open edit, then saved.
        let raw = rawmakase_decode::photo::open(path).unwrap();
        let profiles = rawmakase_decode::camera_look::with_default(
            path,
            &raw.metadata,
            rawmakase_model::camera_profiles::installed(&raw.metadata).0,
            Cancel::NEVER,
        );
        let image = raw
            .develop(rawmakase_model::camera_data::Decode::Half, Cancel::NEVER)
            .unwrap();
        let mut edit = crate::edit::OpenEdit::load(
            library.clone(),
            photo.id,
            photo.path.clone().into(),
            image.metadata.clone(),
            &profiles,
        )
        .unwrap();
        edit.change("Exposure", "", |r| {
            r.exposure = 0.6;
            r.contrast = 0.25;
            r.shadows = 0.3;
            r.vibrance = 0.2;
            r.crop = [0.08, 0.1, 0.9, 0.86];
        });
        edit.change("Radial Gradient", "", |r| {
            crate::masks::new_mask(
                &image,
                r,
                crate::masks::NewMask::Radial,
                [0.5, 0.5],
                [0.7, 0.65],
            )
            .unwrap();
        });
        edit.change("Heal", "", |r| {
            crate::retouch::add_spot(&image, r, &[[0.3, 0.3]], 0.02, [1., 0.75], false, 0.5, 1.)
                .unwrap();
        });
        edit.save().unwrap();
        drop(edit);

        let ours = library
            .clone()
            .export_photos(
                vec![photo.id],
                ExportChoice {
                    format: ExportFormat::Png,
                    quality: 100,
                    long_edge: 0,
                    ..ExportChoice::default()
                },
                dir.path().join("ours").display().to_string(),
            )
            .unwrap()
            .files;
        let theirs = dir.path().join("theirs.png");
        let catalog = data.join("Catalog").join("Photos.rawmakase");
        let status = std::process::Command::new(&bin)
            .args(["render", "--catalog"])
            .arg(&catalog)
            .arg(path)
            .arg(&theirs)
            .status()
            .unwrap();
        assert!(status.success(), "the desktop renders the photo");
        assert!(
            pixels(Path::new(&ours[0])) == pixels(&theirs),
            "the desktop renders the photo differently"
        );
        // Without the catalog the desktop renders its defaults: the edit is
        // what made the two agree.
        let plain = dir.path().join("plain.png");
        let status = std::process::Command::new(&bin)
            .args(["render", "--max-edge", "600"])
            .arg(path)
            .arg(&plain)
            .status()
            .unwrap();
        assert!(status.success());
        let unedited = image::image_dimensions(&plain).unwrap();
        let edited = image::image_dimensions(&theirs).unwrap();
        assert_ne!(unedited, edited, "the crop came through");
    }
}
