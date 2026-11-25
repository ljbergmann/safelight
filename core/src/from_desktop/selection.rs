//! What a Select Subject, Select Background or Select Sky run is given and what
//! the raster it makes records: the input's size, the models' identity, the
//! keys that name an input and a prompt on it, and the runtime they load with.
//! Every front end makes a selection with these, so a mask made in one reads
//! the same in another. Requests, threads and the remote worker are the
//! callers'.
use rawmakase_inference::LoadOptions;
use rawmakase_model::storage::{FNV_OFFSET, fnv1a};
use rawmakase_selection::{Prompt, RgbImage, SKY, SUBJECT};

/// The longest side of the photo rendered for the model. The network looks at
/// 1024 pixels; more only makes the stored raster larger.
pub const INPUT_EDGE: u32 = 1280;

/// Which network a job's answer comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelId {
    Subject,
    Sky,
}
impl ModelId {
    /// Its name, its weights' digest and the versions of its contract and processing.
    fn identity(self) -> (&'static str, &'static str, u32, u32) {
        match self {
            Self::Subject => (
                SUBJECT.id,
                SUBJECT.files[1].sha256,
                SUBJECT.version,
                SUBJECT.processing_version,
            ),
            Self::Sky => (SKY.id, SKY.file.sha256, SKY.version, SKY.processing_version),
        }
    }
    /// What a generated raster records as having made it.
    pub fn provenance(self) -> String {
        let (id, digest, ..) = self.identity();
        format!("{id}@{}", &digest[..16])
    }
    /// What an answer depends on besides the pixels: the model, its weights and
    /// the versions of its contract and processing, as the raster's input
    /// identity records them (the AI Masks extension's `ModelId::key`).
    pub fn key(self) -> String {
        let (id, digest, version, processing) = self.identity();
        format!("{id}|{digest}|{version}|{processing}")
    }
    /// The mask generator that makes it, as the desktop names it: Background is
    /// Subject's, inverted.
    pub fn generator(self) -> &'static str {
        match self {
            Self::Subject => "subject",
            Self::Sky => "sky",
        }
    }
}

/// The desktop's digest of a selection's image: its size, its pixels and the
/// version of how they were rendered (`SELECTION_INPUT_VERSION`), as
/// `mask_generator::image_key` makes it.
pub fn image_key(rgb: &RgbImage) -> String {
    let header = [
        (rgb.width as u32).to_le_bytes(),
        (rgb.height as u32).to_le_bytes(),
        rawmakase_engine::develop::masks::SELECTION_INPUT_VERSION.to_le_bytes(),
    ]
    .concat();
    let hash = |seed| fnv1a(fnv1a(seed, &header), &rgb.data);
    format!("{:016x}{:016x}", hash(FNV_OFFSET), hash(0x84222325cbf29ce4))
}

/// What a raster records as made from: the image (its [`image_key`]), the
/// model, the generator and the clicks, or `None` for an automatic selection,
/// as the desktop's `mask_generator::input_identity` writes it. Saved values are
/// only read, never recomputed.
pub fn input_identity(image_key: &str, model: ModelId, prompt: Option<&Prompt>) -> String {
    let source = format!(
        "{:016x}",
        fnv1a(
            fnv1a(FNV_OFFSET, image_key.as_bytes()),
            model.key().as_bytes()
        )
    );
    let mut text = format!("{source}|{}", model.generator());
    match prompt {
        None => text.push_str("|auto"),
        Some(prompt) => {
            text.push_str("|prompt");
            for p in &prompt.points {
                text.push_str(&format!("|{:.4},{:.4},{}", p.x, p.y, p.positive));
            }
            if let Some(b) = prompt.bounds {
                text.push_str(&format!("|{b:.4?}"));
            }
        }
    }
    format!("{:016x}", fnv1a(FNV_OFFSET, text.as_bytes()))
}

/// How a selection model loads here: with the runtime installed in the runtime
/// folder when there is one, else where `rawmakase-inference` looks.
pub fn load_options() -> LoadOptions {
    let bundled = crate::models::runtime_dir();
    let runtime_library = rawmakase_inference::runtime::runtime_file_names()
        .iter()
        .map(|name| bundled.join(name))
        .find(|path| path.is_file());
    LoadOptions {
        runtime_library,
        threads: None,
        // Selections made on this computer run on its CPU, as before; the
        // accelerators are the remote worker's concern.
        provider: None,
        ..Default::default()
    }
}

/// Whether an ONNX Runtime library this copy can load is in a place it looks.
pub fn runtime_present() -> bool {
    let names = rawmakase_inference::runtime::runtime_file_names();
    let mut dirs = rawmakase_inference::runtime::runtime_search_dirs();
    dirs.push(crate::models::runtime_dir());
    dirs.iter()
        .any(|dir| names.iter().any(|name| dir.join(name).is_file()))
        || std::env::var_os(rawmakase_inference::RUNTIME_ENV).is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identity the desktop's `input_identity` writes for the same image,
    /// model key and clicks: the source hash, the generator, the clicks.
    #[test]
    fn a_raster_records_its_input_as_the_desktop_does() {
        let image = "00112233445566778899aabbccddeeff";
        let source = format!(
            "{:016x}",
            fnv1a(
                fnv1a(FNV_OFFSET, image.as_bytes()),
                ModelId::Subject.key().as_bytes()
            )
        );
        let prompt = Prompt {
            points: vec![rawmakase_selection::Point {
                x: 0.25,
                y: 0.5,
                positive: true,
            }],
            bounds: None,
        };
        let text = format!("{source}|subject|prompt|0.2500,0.5000,true");
        assert_eq!(
            input_identity(image, ModelId::Subject, Some(&prompt)),
            format!("{:016x}", fnv1a(FNV_OFFSET, text.as_bytes()))
        );
        let text = format!("{source}|subject|auto");
        assert_eq!(
            input_identity(image, ModelId::Subject, None),
            format!("{:016x}", fnv1a(FNV_OFFSET, text.as_bytes()))
        );
    }

    #[test]
    fn sky_and_subject_answers_never_share_a_key_or_a_provenance() {
        let rgb = RgbImage {
            width: 2,
            height: 1,
            data: vec![1, 2, 3, 4, 5, 6],
        };
        let image = image_key(&rgb);
        assert_ne!(
            input_identity(&image, ModelId::Sky, None),
            input_identity(&image, ModelId::Subject, None)
        );
        assert!(
            ModelId::Sky
                .provenance()
                .starts_with("upernet-convnext-base@")
        );
        assert!(
            ModelId::Subject
                .provenance()
                .starts_with("sam2.1-hiera-small@")
        );
    }
}
