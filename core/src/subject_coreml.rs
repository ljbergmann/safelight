//! Select Subject and Background with Core ML on the GPU: SAM 2.1 small's image encoder
//! and DETR's panoptic segmentation converted to a half-precision Core ML model
//! (build/Subject-CoreML.mlpackage, made by scripts/convert-subject-coreml.py and bundled
//! with the app). They are nearly all of Select Subject's time; SAM 2's decoder, about
//! 10 ms a call, runs on the CPU from the downloaded model, through RAWmakase's
//! [`Session::analyze_from`](rawmakase_inference::Session::analyze_from).
//!
//! Not on the Neural Engine: on an M1 iPad, loading either network for it takes the app
//! down, and keeps the Neural Engine's compiler busy for minutes after.

use crate::coreml::{self, Input, Model, Units, Values};
use rawmakase_inference::panoptic::Labels;
use rawmakase_inference::process::{preprocess, preprocess_sized};
use rawmakase_inference::runtime::{panoptic_input_size, panoptic_labels};
use rawmakase_inference::{Embedding, InferenceError, PANOPTIC, RgbImage, SUBJECT};
use std::path::PathBuf;

/// The compiled model's name in the app bundle.
const BUNDLED: &str = "Subject-CoreML";

/// Where the networks run.
const UNITS: Units = Units::CPUAndGPU;

/// The bundled model, its encoder and one shape's DETR loaded at a time.
pub(crate) struct CoreMLSubject {
    path: PathBuf,
    encoder: Option<Model>,
    detr: Option<((usize, usize), Model)>,
    /// DETR's shapes, from the model's metadata once the encoder has loaded.
    shapes: Vec<(usize, usize)>,
    /// The model would not load, or was made from other files: the CPU is used.
    refused: bool,
}

impl CoreMLSubject {
    /// The model bundled with the app, if it has one.
    pub(crate) fn bundled() -> Option<Self> {
        Some(Self::at(coreml::bundled(BUNDLED)?.to_file_path()?))
    }

    /// A compiled model (`.mlmodelc`) at `path`.
    pub(crate) fn at(path: PathBuf) -> Self {
        Self {
            path,
            encoder: None,
            detr: None,
            shapes: Vec::new(),
            refused: false,
        }
    }

    /// What [`Session::analyze_from`](rawmakase_inference::Session::analyze_from) takes for
    /// `image`: SAM 2's embedding and DETR's labels, or `None` when the model has no DETR
    /// for the photo's shape or will not load.
    pub(crate) fn inputs(
        &mut self,
        image: &RgbImage,
    ) -> Result<Option<(Embedding, Labels)>, InferenceError> {
        if self.refused {
            return Ok(None);
        }
        let Some(embedding) = self.embed(image)? else {
            return Ok(None);
        };
        let Some(labels) = self.labels(image)? else {
            return Ok(None);
        };
        Ok(Some((embedding, labels)))
    }

    fn embed(&mut self, image: &RgbImage) -> Result<Option<Embedding>, InferenceError> {
        let side = SUBJECT.input_size;
        let tensor = preprocess(image, side, SUBJECT.mean, SUBJECT.std)?;
        if self.encoder.is_none() {
            let Some(model) = self.load("encoder") else {
                self.refused = true;
                return Ok(None);
            };
            self.shapes = detr_shapes(&model);
            self.encoder = Some(model);
        }
        let model = self.encoder.as_mut().expect("loaded above");
        let input = Input {
            name: "pixel_values",
            shape: &[1, 3, side, side],
            values: Values::F32(&tensor),
        };
        let features = model.run(
            &[input],
            &[
                "image_embeddings_0",
                "image_embeddings_1",
                "image_embeddings_2",
            ],
        )?;
        if !features
            .iter()
            .all(|(_, v)| v.iter().all(|x| x.is_finite()))
        {
            return Err(InferenceError::OutputInvalid(
                "SAM 2's encoder answered with values that are not finite".into(),
            ));
        }
        let features = features
            .into_iter()
            .map(|(shape, values)| (shape.iter().map(|&d| d as i64).collect(), values))
            .collect();
        Embedding::from_features(features, image.clone()).map(Some)
    }

    fn labels(&mut self, image: &RgbImage) -> Result<Option<Labels>, InferenceError> {
        // The photo's own shape: resizing it by a pixel moves DETR's segments across its
        // threshold (scripts/convert-subject-coreml.py).
        let shape = panoptic_input_size(image.width, image.height);
        if !self.shapes.contains(&shape) {
            return Ok(None);
        }
        let (width, height) = shape;
        let tensor = preprocess_sized(image, width, height, PANOPTIC.mean, PANOPTIC.std)?;
        if self.detr.as_ref().is_none_or(|(s, _)| *s != shape) {
            // One shape at a time: each brings DETR's weights into memory.
            self.detr = None;
            let Some(model) = self.load(&format!("detr_{width}x{height}")) else {
                return Ok(None);
            };
            self.detr = Some((shape, model));
        }
        let (_, model) = self.detr.as_mut().expect("loaded above");
        // Every pixel is real: the model takes this mask at a fixed size.
        let valid = vec![1i32; 64 * 64];
        let inputs = [
            Input {
                name: "pixel_values",
                shape: &[1, 3, height, width],
                values: Values::F32(&tensor),
            },
            Input {
                name: "pixel_mask",
                shape: &[1, 64, 64],
                values: Values::I32(&valid),
            },
        ];
        let answer = model.run(&inputs, &["logits", "pred_masks"])?;
        if !answer.iter().all(|(_, v)| v.iter().all(|x| x.is_finite())) {
            return Err(InferenceError::OutputInvalid(
                "DETR answered with values that are not finite".into(),
            ));
        }
        let dims = |s: &[usize]| s.iter().map(|&d| d as i64).collect::<Vec<_>>();
        let (logits, masks) = (&answer[0], &answer[1]);
        panoptic_labels(&dims(&logits.0), &logits.1, &dims(&masks.0), &masks.1).map(Some)
    }

    /// `function` of the model, if it loads and was made from RAWmakase's files.
    fn load(&self, function: &str) -> Option<Model> {
        let url = coreml::at(&self.path)?;
        let model = Model::load(&url, UNITS, Some(function)).ok()?;
        let encoder = SUBJECT
            .files
            .iter()
            .find(|f| f.name == "vision_encoder.onnx_data")?
            .sha256;
        let made_from = |key: &str, sha: &str| model.metadata(key).ok().as_deref() == Some(sha);
        (made_from("encoder_sha256", encoder) && made_from("detr_sha256", PANOPTIC.file.sha256))
            .then_some(model)
    }
}

/// The DETR shapes the model's metadata lists.
fn detr_shapes(model: &Model) -> Vec<(usize, usize)> {
    model
        .metadata("detr_functions")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .collect()
}

/// Compares Select Subject's masks with Core ML with the CPU's on real
/// photos. Needs `SAFELIGHT_SUBJECT_ONNX` (RAWmakase's sam2.1-hiera-small-v1 files),
/// `SAFELIGHT_SUBJECT_PHOTOS` (a folder of JPEGs with people or animals),
/// `RAWMAKASE_ORT_LIB` and build/Subject-CoreML.mlpackage.
#[cfg(test)]
mod compare {
    use super::*;
    use rawmakase_cancel::Cancel;
    use rawmakase_inference::{Coverage, LoadOptions, Subject};
    use std::path::Path;

    fn iou(a: &Coverage, b: &Coverage) -> f64 {
        let (mut both, mut either) = (0usize, 0usize);
        for (x, y) in a.data.iter().zip(&b.data) {
            both += usize::from(*x >= 128 && *y >= 128);
            either += usize::from(*x >= 128 || *y >= 128);
        }
        if either == 0 {
            1.
        } else {
            both as f64 / either as f64
        }
    }

    #[test]
    #[ignore = "needs the Subject model files, photos, ONNX Runtime and build/Subject-CoreML.mlpackage"]
    fn masks_with_core_ml_agree_with_the_cpus() {
        let dir = tempfile::tempdir().unwrap();
        let package =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/Subject-CoreML.mlpackage");
        assert!(
            std::process::Command::new("xcrun")
                .args(["coremlcompiler", "compile"])
                .arg(&package)
                .arg(dir.path())
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
        let mut neural = CoreMLSubject::at(dir.path().join("Subject-CoreML.mlmodelc"));
        let onnx = std::env::var("SAFELIGHT_SUBJECT_ONNX").unwrap();
        let mut cpu = Subject::load_with(Path::new(&onnx), &LoadOptions::default()).unwrap();
        let mut photos: Vec<_> =
            std::fs::read_dir(std::env::var("SAFELIGHT_SUBJECT_PHOTOS").unwrap())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "jpg"))
                .collect();
        photos.sort();
        for photo in photos {
            let image = image::open(&photo).unwrap().into_rgb8();
            let image = image::imageops::resize(
                &image,
                1280,
                1280 * image.height() / image.width(),
                image::imageops::FilterType::Triangle,
            );
            let rgb = RgbImage {
                width: image.width() as usize,
                height: image.height() as usize,
                data: image.into_raw(),
            };
            let reference = cpu.analyze(&rgb, Cancel::NEVER).unwrap().subject().unwrap();
            let (embedding, labels) = neural
                .inputs(&rgb)
                .unwrap()
                .expect("a function for the photo");
            let detr_only = {
                let cpu_embedding = cpu.embed(&rgb, Cancel::NEVER).unwrap();
                let (_, labels) = neural.inputs(&rgb).unwrap().unwrap();
                cpu.analyze_from(cpu_embedding, labels, Cancel::NEVER)
                    .unwrap()
                    .subject()
                    .unwrap()
            };
            let both = cpu
                .analyze_from(embedding, labels, Cancel::NEVER)
                .unwrap()
                .subject()
                .unwrap();
            println!(
                "{}: both IoU {:.4}, DETR only IoU {:.4}",
                photo.file_name().unwrap().to_string_lossy(),
                iou(&reference, &both),
                iou(&reference, &detr_only)
            );
        }
    }
}
