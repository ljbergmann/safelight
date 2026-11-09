//! Select Sky on the Neural Engine: UperNet ConvNeXt-base converted to a half-precision
//! Core ML model with one function per photo shape (build/Sky-ANE.mlpackage, made by
//! scripts/convert-sky-ane.py and bundled with the app). A photo whose shape has no
//! function, or a model that fails, is left to the ONNX model on the CPU, as before.

use crate::coreml::{self, Model, Units};
use rawmakase_inference::process::preprocess_sized;
use rawmakase_inference::runtime::sky_input_size;
use rawmakase_inference::{Coverage, InferenceError, RgbImage, SKY};
use std::collections::HashSet;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;

/// The compiled model's name in the app bundle.
const BUNDLED: &str = "Sky-ANE";

/// The bundled model, one shape's function loaded at a time: each brings its own copy of
/// the weights (about 233 MB) into memory.
pub(crate) struct NeuralSky {
    path: PathBuf,
    loaded: Option<((usize, usize), Units, Model)>,
    /// Shapes with no function in the model, or whose function would not load.
    missing: HashSet<(usize, usize)>,
}

impl NeuralSky {
    /// The model bundled with the app, if it has one.
    pub(crate) fn bundled() -> Option<Self> {
        let url = coreml::bundled(BUNDLED)?;
        Some(Self::at(url.to_file_path()?))
    }

    /// A compiled model (`.mlmodelc`) at `path`.
    pub(crate) fn at(path: PathBuf) -> Self {
        Self {
            path,
            loaded: None,
            missing: HashSet::new(),
        }
    }

    /// The photo's sky, as [`rawmakase_inference::SkySession::select`] answers it, or
    /// `None` when the model has no function for the photo's shape. On the Neural Engine,
    /// and on the GPU when the Neural Engine fails.
    pub(crate) fn select(&mut self, image: &RgbImage) -> Result<Option<Coverage>, InferenceError> {
        let (width, height) = sky_input_size(image.width, image.height);
        let input = preprocess_sized(image, width, height, SKY.mean, SKY.std)?;
        let mut answer = None;
        for units in [Units::CPUAndNeuralEngine, Units::CPUAndGPU] {
            let Some(model) = self.function(width, height, units) else {
                return Ok(None);
            };
            let run = model.predict(
                "pixel_values",
                &[1, 3, height, width],
                |data| data.copy_from_slice(&input),
                "sky",
                &[1, height, width],
                1.,
            );
            match run {
                Ok(mut probability) if probability.iter().all(|p| p.is_finite()) => {
                    // Half precision can land a hair outside 0..=1.
                    for p in &mut probability {
                        *p = p.clamp(0., 1.);
                    }
                    answer = Some(probability);
                    break;
                }
                // Not finite, or failed: the same function on the next units.
                _ => self.loaded = None,
            }
        }
        let Some(probability) = answer else {
            return Err(InferenceError::Failed(
                "the sky model failed on the Neural Engine and the GPU".into(),
            ));
        };
        rawmakase_inference::auto::sky(image, &probability, width, height).map(Some)
    }

    /// The function for `width` x `height` on `units`, loaded if it is not, or `None`
    /// when the model has none (or it will not load, or was made from another model).
    fn function(&mut self, width: usize, height: usize, units: Units) -> Option<&mut Model> {
        let shape = (width, height);
        if self.missing.contains(&shape) {
            return None;
        }
        if self
            .loaded
            .as_ref()
            .is_none_or(|(s, u, _)| (*s, *u) != (shape, units))
        {
            self.loaded = None;
            let url = coreml::at(&self.path)?;
            let model = Model::load(&url, units, Some(&format!("sky_{width}x{height}")))
                .ok()
                .filter(|m| m.metadata("source_sha256").ok().as_deref() == Some(SKY.file.sha256));
            match model {
                Some(model) => self.loaded = Some((shape, units, model)),
                None => {
                    self.missing.insert(shape);
                    return None;
                }
            }
        }
        self.loaded.as_mut().map(|(_, _, m)| m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawmakase_inference::process::output_size;

    /// A photo-like test image: a blue sky over darker, textured ground.
    fn landscape(width: usize, height: usize) -> RgbImage {
        let mut data = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                let pixel = if y < height / 2 {
                    [110, 160, 230]
                } else {
                    let t = ((x * 31 + y * 17) % 40) as u8;
                    [60 + t, 70 + t, 40 + t]
                };
                data.extend(pixel);
            }
        }
        RgbImage {
            width,
            height,
            data,
        }
    }

    #[test]
    #[ignore = "needs build/Sky-ANE.mlpackage (scripts/convert-sky-ane.py)"]
    fn a_covered_shape_runs_on_the_model_and_another_is_left_to_the_cpu() {
        let dir = tempfile::tempdir().unwrap();
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../build/Sky-ANE.mlpackage");
        let status = std::process::Command::new("xcrun")
            .args(["coremlcompiler", "compile"])
            .arg(&package)
            .arg(dir.path())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "coremlcompiler failed");
        let mut sky = NeuralSky::at(dir.path().join("Sky-ANE.mlmodelc"));
        let photo = landscape(1536, 1024);
        let coverage = sky.select(&photo).unwrap().expect("3:2 has a function");
        assert_eq!((coverage.width, coverage.height), output_size(&photo));
        let top = coverage.data[coverage.width * coverage.height / 8];
        let bottom = coverage.data[coverage.width * coverage.height * 7 / 8];
        assert!(top > 128 && bottom < 128, "sky {top}, ground {bottom}");
        // A panorama's proportions have no function: the CPU's model takes it.
        assert!(sky.select(&landscape(2000, 400)).unwrap().is_none());
        // And the 3:2 function is still served after it.
        assert!(sky.select(&photo).unwrap().is_some());
    }
}
