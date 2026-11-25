//! AI Denoise on the Neural Engine: RawNIND converted to a half-precision Core ML model
//! (app/Models/RawNIND-ANE.mlpackage, made by scripts/convert-denoise-ane.py), run by
//! Core ML directly. ONNX Runtime cannot put the network there: its CoreML provider
//! keeps it in single precision, which the Neural Engine does not compute.
//!
//! The conversion rescales each layer so its values fit half precision. The model
//! therefore takes the input multiplied by `input_scale` and answers the output
//! multiplied by `output_scale`; both are read from its metadata, with the SHA-256 of
//! the ONNX model it was made from, which must be the one RAWmakase installs.

use crate::coreml::{self, Model, Units};
use rawmakase_cancel::Cancel;
use rawmakase_denoise::Loader;
use rawmakase_inference::denoise::TileModel;
use rawmakase_inference::{Denoiser, InferenceError, LoadOptions, Provider, RAWNIND};
use std::path::Path;
use std::sync::Arc;

/// The compiled model's name in the app bundle (Xcode compiles the `.mlpackage`).
const BUNDLED: &str = "RawNIND-ANE";

/// The model on the Neural Engine. One tile at a time.
pub(crate) struct NeuralDenoiser {
    model: Model,
    input_scale: f32,
    output_scale: f32,
}

impl NeuralDenoiser {
    /// The model bundled with the app.
    pub(crate) fn bundled() -> Result<Self, String> {
        let url =
            coreml::bundled(BUNDLED).ok_or_else(|| format!("the app has no {BUNDLED}.mlmodelc"))?;
        Self::open(Model::load(&url, Units::CPUAndNeuralEngine, None)?)
    }

    /// A compiled model (`.mlmodelc`) at `path`.
    #[cfg(test)]
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let url = coreml::at(path).ok_or_else(|| format!("{} is not a path", path.display()))?;
        Self::open(Model::load(&url, Units::CPUAndNeuralEngine, None)?)
    }

    fn open(model: Model) -> Result<Self, String> {
        let source = model.metadata("source_sha256")?;
        if source != RAWNIND.model.sha256 {
            return Err(format!(
                "the Neural Engine model was made from {source}, not the installed model"
            ));
        }
        let scale = |key: &str| -> Result<f32, String> {
            let v: f32 = model
                .metadata(key)?
                .parse()
                .map_err(|_| format!("{key} is not a number"))?;
            (v.is_finite() && v > 0.)
                .then_some(v)
                .ok_or_else(|| format!("{key} is {v}"))
        };
        Ok(Self {
            input_scale: scale("input_scale")?,
            output_scale: scale("output_scale")?,
            model,
        })
    }
}

impl TileModel for NeuralDenoiser {
    fn run(&mut self, input: &[f32], cancel: Cancel<'_>) -> Result<Vec<f32>, InferenceError> {
        if cancel.is_cancelled() {
            return Err(InferenceError::Cancelled);
        }
        let side = RAWNIND.tile;
        if input.len() != 4 * side * side {
            return Err(InferenceError::OutputInvalid(format!(
                "a tile of {} values, expected {}",
                input.len(),
                4 * side * side
            )));
        }
        let scale = self.input_scale;
        self.model.predict(
            RAWNIND.input,
            &[1, 4, side, side],
            |data| {
                for (d, v) in data.iter_mut().zip(input) {
                    *d = v * scale;
                }
            },
            RAWNIND.output,
            &[1, 3, 2 * side, 2 * side],
            1. / self.output_scale,
        )
    }
}

/// The provider AI Denoise asks for: the Neural Engine on an iPad or iPhone, as
/// [`loader`] runs it, unless a Neural Engine load has outlasted its limit in this
/// process (then CoreML's GPU); elsewhere RAWmakase's choice.
pub(crate) fn provider() -> Provider {
    if cfg!(target_os = "ios") && coreml::neural_engine_stalled() {
        Provider::CoreML
    } else if cfg!(target_os = "ios") {
        Provider::NeuralEngine
    } else {
        rawmakase_denoise::preferred_provider()
    }
}

/// The installed ONNX model in `dir` on `provider`, through ONNX Runtime, with the
/// library in [`crate::models::runtime_dir`] when it is there. Fewer threads than
/// rendering's, as RAWmakase's own loader takes, so a job leaves the editor room.
pub(crate) fn load_onnx(
    dir: &Path,
    provider: Provider,
) -> Result<Box<dyn TileModel + Send>, String> {
    let runtime = crate::models::runtime_dir();
    let runtime_library = rawmakase_inference::runtime::runtime_file_names()
        .iter()
        .map(|name| runtime.join(name))
        .find(|path| path.is_file());
    Denoiser::load_with(
        dir,
        &LoadOptions {
            runtime_library,
            threads: Some(
                rawmakase_inference::runtime::default_threads()
                    .saturating_sub(2)
                    .max(2),
            ),
            provider: Some(provider),
            ..Default::default()
        },
    )
    .map(|d| Box::new(d) as Box<dyn TileModel + Send>)
    .map_err(|e| e.to_string())
}

/// Opens the RAW at `path` and packs its mosaic for AI Denoise, as RAWmakase's
/// service does.
pub(crate) fn source() -> rawmakase_denoise::Source {
    Arc::new(|path: &Path, cancel: Cancel<'_>| {
        if !rawmakase_model::storage::is_raw(path) {
            return Err(anyhow::Error::new(
                rawmakase_decode::ai::denoise::Unsupported("AI Denoise works on RAW files only."),
            ));
        }
        rawmakase_decode::photo::open(path)?.packed_bayer(cancel)
    })
}

/// AI Denoise's model loader: the bundled model on [`Provider::NeuralEngine`], and
/// ONNX Runtime with the installed model otherwise. A tile that comes out not finite
/// moves the job to the CPU (RAWmakase's service does that); a bundled model that won't
/// load fails the job, and once a load has outlasted its limit the next service asks
/// for CoreML's GPU instead ([`provider`]).
pub(crate) fn loader() -> Loader {
    Arc::new(|dir: &Path, provider: Provider| match provider {
        Provider::NeuralEngine => {
            NeuralDenoiser::bundled().map(|d| Box::new(d) as Box<dyn TileModel + Send>)
        }
        other => load_onnx(dir, other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed model, compiled as Xcode compiles it for the app.
    fn compiled() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let package =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/Models/RawNIND-ANE.mlpackage");
        let status = std::process::Command::new("xcrun")
            .args(["coremlcompiler", "compile"])
            .arg(&package)
            .arg(dir.path())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "coremlcompiler failed");
        let path = dir.path().join("RawNIND-ANE.mlmodelc");
        (dir, path)
    }

    #[test]
    fn the_bundled_model_is_made_from_the_installed_one_and_answers_a_tile() {
        let (_dir, path) = compiled();
        let mut model = NeuralDenoiser::load(&path).unwrap();
        assert!(model.input_scale > 1. && model.output_scale < 1.);
        let side = RAWNIND.tile;
        // A smooth ramp, as a dim photo's mosaic.
        let input: Vec<f32> = (0..4 * side * side)
            .map(|i| (i % side) as f32 / side as f32 * 0.2)
            .collect();
        let out = model.run(&input, Cancel::NEVER).unwrap();
        assert_eq!(out.len(), 3 * 4 * side * side);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(out.iter().any(|v| *v != 0.));
    }

    #[test]
    fn a_cancelled_run_never_answers() {
        let (_dir, path) = compiled();
        let mut model = NeuralDenoiser::load(&path).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let input = vec![0.; 4 * RAWNIND.tile * RAWNIND.tile];
        assert!(matches!(
            model.run(&input, Cancel::new(&cancel)),
            Err(InferenceError::Cancelled)
        ));
    }

    #[test]
    fn a_tile_of_the_wrong_size_is_refused() {
        let (_dir, path) = compiled();
        let mut model = NeuralDenoiser::load(&path).unwrap();
        assert!(matches!(
            model.run(&[0.; 4], Cancel::NEVER),
            Err(InferenceError::OutputInvalid(_))
        ));
    }
}
