//! AI Denoise on the Neural Engine: RawNIND converted to a half-precision Core ML model
//! (app/Models/RawNIND-ANE.mlpackage, made by scripts/convert-denoise-ane.py), run by
//! Core ML directly. ONNX Runtime cannot put the network there: its CoreML provider
//! keeps it in single precision, which the Neural Engine does not compute.
//!
//! The conversion rescales each layer so its values fit half precision. The model
//! therefore takes the input multiplied by `input_scale` and answers the output
//! multiplied by `output_scale`; both are read from its metadata, with the SHA-256 of
//! the ONNX model it was made from, which must be the one RAWmakase installs.

use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue, MLModel,
    MLModelConfiguration, MLModelCreatorDefinedKey, MLMultiArray, MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSError, NSNumber, NSString, NSURL};
use rawmakase_ai::ai_denoise::{Loader, load_model};
use rawmakase_cancel::Cancel;
use rawmakase_inference::denoise::TileModel;
use rawmakase_inference::{InferenceError, Provider, RAWNIND};
use std::path::Path;
use std::sync::Arc;

/// The compiled model's name in the app bundle (Xcode compiles the `.mlpackage`).
const BUNDLED: &str = "RawNIND-ANE";

/// The model on the Neural Engine. One tile at a time.
pub(crate) struct NeuralDenoiser {
    model: Retained<MLModel>,
    input_scale: f32,
    output_scale: f32,
}

// SAFETY: Core ML's objects may be used from any thread. A `NeuralDenoiser` is used
// by one thread at a time (`TileModel::run` takes `&mut self`); `Send` only lets the
// AI Denoise worker take it.
unsafe impl Send for NeuralDenoiser {}

impl NeuralDenoiser {
    /// The model bundled with the app.
    pub(crate) fn bundled() -> Result<Self, String> {
        let url = NSBundle::mainBundle()
            .URLForResource_withExtension(
                Some(&NSString::from_str(BUNDLED)),
                Some(&NSString::from_str("mlmodelc")),
            )
            .ok_or_else(|| format!("the app has no {BUNDLED}.mlmodelc"))?;
        Self::open(&url)
    }

    /// A compiled model (`.mlmodelc`) at `path`.
    #[cfg(test)]
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let url = NSURL::from_file_path(path)
            .ok_or_else(|| format!("{} is not a path", path.display()))?;
        Self::open(&url)
    }

    fn open(url: &NSURL) -> Result<Self, String> {
        autoreleasepool(|_| {
            let config = unsafe { MLModelConfiguration::new() };
            unsafe { config.setComputeUnits(MLComputeUnits::CPUAndNeuralEngine) };
            let model =
                unsafe { MLModel::modelWithContentsOfURL_configuration_error(url, &config) }
                    .map_err(|e| message(&e))?;
            let meta = |key: &str| -> Result<String, String> {
                let all = unsafe { model.modelDescription().metadata() };
                let creator = unsafe { MLModelCreatorDefinedKey }
                    .and_then(|k| all.objectForKey(k))
                    .and_then(|d| d.downcast::<NSDictionary>().ok())
                    .ok_or("the model has no metadata of its maker")?;
                creator
                    .objectForKey(&NSString::from_str(key))
                    .and_then(|v| v.downcast::<NSString>().ok())
                    .map(|v| v.to_string())
                    .ok_or_else(|| format!("the model's metadata has no {key}"))
            };
            let source = meta("source_sha256")?;
            if source != RAWNIND.model.sha256 {
                return Err(format!(
                    "the Neural Engine model was made from {source}, not the installed model"
                ));
            }
            let scale = |key: &str| -> Result<f32, String> {
                let v: f32 = meta(key)?
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
        })
    }
}

// `dataPointer` is deprecated for `getMutableBytesWithHandler`, which lends the bytes
// to a block. The arrays here are made or answered for this call alone and read at
// once, on this thread, so the pointer is as safe and simpler.
#[allow(deprecated)]
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
        autoreleasepool(|_| {
            let array = new_array(&[1, 4, side, side])?;
            // SAFETY: a new array of this shape is contiguous `f32`s, as many as `input`.
            let data = unsafe {
                std::slice::from_raw_parts_mut(
                    array.dataPointer().cast::<f32>().as_ptr(),
                    input.len(),
                )
            };
            for (d, v) in data.iter_mut().zip(input) {
                *d = v * self.input_scale;
            }
            let value = unsafe { MLFeatureValue::featureValueWithMultiArray(&array) };
            let key = NSString::from_str(RAWNIND.input);
            let features =
                NSDictionary::<NSString, AnyObject>::from_slices(&[&*key], &[value.as_ref()]);
            let provider = unsafe {
                MLDictionaryFeatureProvider::initWithDictionary_error(
                    MLDictionaryFeatureProvider::alloc(),
                    &features,
                )
            }
            .map_err(|e| failed(&e))?;
            let result = unsafe {
                self.model
                    .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
            }
            .map_err(|e| failed(&e))?;
            let output = unsafe { result.featureValueForName(&NSString::from_str(RAWNIND.output)) }
                .and_then(|v| unsafe { v.multiArrayValue() })
                .ok_or_else(|| {
                    InferenceError::ModelInvalid("the model has no output array".into())
                })?;
            read_output(&output, 2 * side, 1. / self.output_scale)
        })
    }
}

#[allow(deprecated)] // `dataPointer`: see `impl TileModel for NeuralDenoiser`.
/// The output `[1, 3, side, side]`, multiplied by `scale`, as contiguous `f32`s. The
/// Neural Engine answers in half precision and may pad rows: read by the strides.
fn read_output(array: &MLMultiArray, side: usize, scale: f32) -> Result<Vec<f32>, InferenceError> {
    let numbers = |list: Retained<NSArray<NSNumber>>| -> Vec<usize> {
        list.iter().map(|n| n.as_usize()).collect()
    };
    let shape = numbers(unsafe { array.shape() });
    if shape != [1, 3, side, side] {
        return Err(InferenceError::OutputInvalid(format!(
            "the model answered with shape {shape:?}"
        )));
    }
    let strides = numbers(unsafe { array.strides() });
    let (sc, sy, sx) = (strides[1], strides[2], strides[3]);
    let len = sc * 2 + sy * (side - 1) + sx * (side - 1) + 1;
    let base = unsafe { array.dataPointer() };
    let mut out = Vec::with_capacity(3 * side * side);
    match unsafe { array.dataType() } {
        MLMultiArrayDataType::Float16 => {
            // SAFETY: the array holds `len` half-precision values by its own strides.
            let data =
                unsafe { std::slice::from_raw_parts(base.cast::<half::f16>().as_ptr(), len) };
            for c in 0..3 {
                for y in 0..side {
                    let row = c * sc + y * sy;
                    out.extend((0..side).map(|x| data[row + x * sx].to_f32() * scale));
                }
            }
        }
        MLMultiArrayDataType::Float32 => {
            // SAFETY: as above, single precision.
            let data = unsafe { std::slice::from_raw_parts(base.cast::<f32>().as_ptr(), len) };
            for c in 0..3 {
                for y in 0..side {
                    let row = c * sc + y * sy;
                    out.extend((0..side).map(|x| data[row + x * sx] * scale));
                }
            }
        }
        other => {
            return Err(InferenceError::OutputInvalid(format!(
                "the model answered in data type {}",
                other.0
            )));
        }
    }
    Ok(out)
}

fn new_array(shape: &[usize]) -> Result<Retained<MLMultiArray>, InferenceError> {
    let shape: Vec<Retained<NSNumber>> = shape.iter().map(|&n| NSNumber::new_usize(n)).collect();
    let shape = NSArray::from_retained_slice(&shape);
    unsafe {
        MLMultiArray::initWithShape_dataType_error(
            MLMultiArray::alloc(),
            &shape,
            MLMultiArrayDataType::Float32,
        )
    }
    .map_err(|e| failed(&e))
}

fn message(e: &NSError) -> String {
    e.localizedDescription().to_string()
}

fn failed(e: &NSError) -> InferenceError {
    InferenceError::Failed(message(e))
}

/// The provider AI Denoise asks for: the Neural Engine on an iPad or iPhone, as
/// [`loader`] runs it; elsewhere RAWmakase's choice.
pub(crate) fn provider() -> Provider {
    if cfg!(target_os = "ios") {
        Provider::NeuralEngine
    } else {
        rawmakase_ai::ai_denoise::preferred_provider()
    }
}

/// AI Denoise's model loader: the bundled model on [`Provider::NeuralEngine`], and
/// ONNX Runtime with the installed model otherwise (the CPU redoes a tile the Neural
/// Engine answered with a value that is not finite).
pub(crate) fn loader() -> Loader {
    Arc::new(|dir: &Path, provider: Provider| match provider {
        Provider::NeuralEngine => {
            NeuralDenoiser::bundled().map(|d| Box::new(d) as Box<dyn TileModel + Send>)
        }
        other => load_model(dir, other),
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
