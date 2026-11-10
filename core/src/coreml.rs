//! Core ML, called directly: the models the app bundles for the Neural Engine
//! (`app/Models/README.md`). One model, one function of it, on the compute units asked
//! for; an input array in, an output array read back as `f32`.

use objc2::AnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_core_ml::{
    MLComputeUnits, MLDictionaryFeatureProvider, MLFeatureProvider, MLFeatureValue,
    MLModel as CoreMLModel, MLModelConfiguration, MLModelCreatorDefinedKey, MLMultiArray,
    MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSError, NSNumber, NSString, NSURL};
use rawmakase_inference::InferenceError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

pub(crate) use objc2_core_ml::MLComputeUnits as Units;

/// A loaded model (or one function of a model with several).
pub(crate) struct Model(Retained<CoreMLModel>);

// SAFETY: Core ML's objects may be used from any thread. A `Model` is used by one
// thread at a time (`predict` takes `&mut self`); `Send` only lets it move to the
// thread that runs it.
unsafe impl Send for Model {}

/// The compiled model `name` in the app bundle (Xcode compiles each `.mlpackage`).
pub(crate) fn bundled(name: &str) -> Option<Retained<NSURL>> {
    NSBundle::mainBundle().URLForResource_withExtension(
        Some(&NSString::from_str(name)),
        Some(&NSString::from_str("mlmodelc")),
    )
}

/// A compiled model (`.mlmodelc`) at `path`.
pub(crate) fn at(path: &std::path::Path) -> Option<Retained<NSURL>> {
    NSURL::from_file_path(path)
}

/// How long a load for the Neural Engine may take. The first load after installing
/// compiles the function for it: 7 to 10 s for Select Sky's on the M1 iPad Pro. When
/// `ANECompilerService` is stuck (as after a model took the app down while it
/// compiled), the load never returns; after this, the GPU is used instead.
const NEURAL_ENGINE_LOAD_LIMIT: Duration = Duration::from_secs(60);

/// Set once a load for the Neural Engine outlasts its limit. The compiler is then
/// stuck until the device restarts, so later loads go straight to the next units.
static NEURAL_ENGINE_STALLED: AtomicBool = AtomicBool::new(false);

/// Whether a load for the Neural Engine has outlasted its limit in this process.
pub(crate) fn neural_engine_stalled() -> bool {
    NEURAL_ENGINE_STALLED.load(Ordering::Relaxed)
}

impl Model {
    /// The model at `url` on `units`; `function`, for a model with several. A load for
    /// the Neural Engine fails after [`NEURAL_ENGINE_LOAD_LIMIT`], and at once for the
    /// rest of the process after one has.
    pub(crate) fn load(
        url: &NSURL,
        units: MLComputeUnits,
        function: Option<&str>,
    ) -> Result<Self, String> {
        if units != MLComputeUnits::CPUAndNeuralEngine && units != MLComputeUnits::All {
            return Self::load_now(url, units, function);
        }
        let path = url
            .to_file_path()
            .ok_or("the model's URL is not a file path")?;
        let function = function.map(str::to_owned);
        within(
            NEURAL_ENGINE_LOAD_LIMIT,
            &NEURAL_ENGINE_STALLED,
            move || {
                let url = at(&path).ok_or("the model's path is not a URL")?;
                Self::load_now(&url, units, function.as_deref())
            },
        )
    }

    fn load_now(
        url: &NSURL,
        units: MLComputeUnits,
        function: Option<&str>,
    ) -> Result<Self, String> {
        autoreleasepool(|_| {
            let config = unsafe { MLModelConfiguration::new() };
            unsafe { config.setComputeUnits(units) };
            if let Some(function) = function {
                unsafe { config.setFunctionName(Some(&NSString::from_str(function))) };
            }
            unsafe { CoreMLModel::modelWithContentsOfURL_configuration_error(url, &config) }
                .map(Self)
                .map_err(|e| message(&e))
        })
    }

    /// A value its maker stored in the model's metadata.
    pub(crate) fn metadata(&self, key: &str) -> Result<String, String> {
        autoreleasepool(|_| {
            let all = unsafe { self.0.modelDescription().metadata() };
            let creator = unsafe { MLModelCreatorDefinedKey }
                .and_then(|k| all.objectForKey(k))
                .and_then(|d| d.downcast::<NSDictionary>().ok())
                .ok_or("the model has no metadata of its maker")?;
            creator
                .objectForKey(&NSString::from_str(key))
                .and_then(|v| v.downcast::<NSString>().ok())
                .map(|v| v.to_string())
                .ok_or_else(|| format!("the model's metadata has no {key}"))
        })
    }

    /// Runs the model on one `f32` input of `shape`, written by `fill`, and answers its
    /// `output` (which must be `expected` in shape) as contiguous `f32`s, each multiplied
    /// by `scale`.
    // `dataPointer` is deprecated for `getMutableBytesWithHandler`, which lends the bytes
    // to a block. The arrays here are made or answered for this call alone and read at
    // once, on this thread, so the pointer is as safe and simpler.
    #[allow(deprecated)]
    pub(crate) fn predict(
        &mut self,
        input: &str,
        shape: &[usize],
        fill: impl FnOnce(&mut [f32]),
        output: &str,
        expected: &[usize],
        scale: f32,
    ) -> Result<Vec<f32>, InferenceError> {
        autoreleasepool(|_| {
            let numbers: Vec<Retained<NSNumber>> =
                shape.iter().map(|&n| NSNumber::new_usize(n)).collect();
            let array = unsafe {
                MLMultiArray::initWithShape_dataType_error(
                    MLMultiArray::alloc(),
                    &NSArray::from_retained_slice(&numbers),
                    MLMultiArrayDataType::Float32,
                )
            }
            .map_err(|e| failed(&e))?;
            // SAFETY: a new array of this shape is contiguous `f32`s, as many as the shape
            // holds.
            fill(unsafe {
                std::slice::from_raw_parts_mut(
                    array.dataPointer().cast::<f32>().as_ptr(),
                    shape.iter().product(),
                )
            });
            let value = unsafe { MLFeatureValue::featureValueWithMultiArray(&array) };
            let key = NSString::from_str(input);
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
                self.0
                    .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
            }
            .map_err(|e| failed(&e))?;
            let answer = unsafe { result.featureValueForName(&NSString::from_str(output)) }
                .and_then(|v| unsafe { v.multiArrayValue() })
                .ok_or_else(|| {
                    InferenceError::ModelInvalid(format!("the model has no output {output}"))
                })?;
            read(&answer, Some(expected), scale).map(|(_, data)| data)
        })
    }

    /// Runs the model on `inputs` and answers each of `outputs`, its shape and its values
    /// as contiguous `f32`s.
    // `dataPointer`: see `predict`.
    #[allow(deprecated)]
    pub(crate) fn run(
        &mut self,
        inputs: &[Input<'_>],
        outputs: &[&str],
    ) -> Result<Vec<Answer>, InferenceError> {
        autoreleasepool(|_| {
            let mut keys = Vec::with_capacity(inputs.len());
            let mut values = Vec::with_capacity(inputs.len());
            for input in inputs {
                let numbers: Vec<Retained<NSNumber>> = input
                    .shape
                    .iter()
                    .map(|&n| NSNumber::new_usize(n))
                    .collect();
                let kind = match input.values {
                    Values::F32(_) => MLMultiArrayDataType::Float32,
                    Values::I32(_) => MLMultiArrayDataType::Int32,
                };
                let array = unsafe {
                    MLMultiArray::initWithShape_dataType_error(
                        MLMultiArray::alloc(),
                        &NSArray::from_retained_slice(&numbers),
                        kind,
                    )
                }
                .map_err(|e| failed(&e))?;
                let count: usize = input.shape.iter().product();
                // SAFETY: a new array of this shape is contiguous values of its type, as
                // many as the shape holds; each input is checked to be as long.
                match input.values {
                    Values::F32(data) if data.len() == count => unsafe {
                        std::slice::from_raw_parts_mut(
                            array.dataPointer().cast::<f32>().as_ptr(),
                            count,
                        )
                    }
                    .copy_from_slice(data),
                    Values::I32(data) if data.len() == count => unsafe {
                        std::slice::from_raw_parts_mut(
                            array.dataPointer().cast::<i32>().as_ptr(),
                            count,
                        )
                    }
                    .copy_from_slice(data),
                    _ => {
                        return Err(InferenceError::OutputInvalid(format!(
                            "the input {} does not fill {:?}",
                            input.name, input.shape
                        )));
                    }
                }
                keys.push(NSString::from_str(input.name));
                values.push(unsafe { MLFeatureValue::featureValueWithMultiArray(&array) });
            }
            let keys: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
            let values: Vec<&AnyObject> = values.iter().map(|v| v.as_ref()).collect();
            let features = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &values);
            let provider = unsafe {
                MLDictionaryFeatureProvider::initWithDictionary_error(
                    MLDictionaryFeatureProvider::alloc(),
                    &features,
                )
            }
            .map_err(|e| failed(&e))?;
            let result = unsafe {
                self.0
                    .predictionFromFeatures_error(ProtocolObject::from_ref(&*provider))
            }
            .map_err(|e| failed(&e))?;
            outputs
                .iter()
                .map(|output| {
                    let answer = unsafe { result.featureValueForName(&NSString::from_str(output)) }
                        .and_then(|v| unsafe { v.multiArrayValue() })
                        .ok_or_else(|| {
                            InferenceError::ModelInvalid(format!(
                                "the model has no output {output}"
                            ))
                        })?;
                    read(&answer, None, 1.)
                })
                .collect()
        })
    }
}

/// One output of [`Model::run`]: its shape, and its values as contiguous `f32`s.
pub(crate) type Answer = (Vec<usize>, Vec<f32>);

/// One input of [`Model::run`].
pub(crate) struct Input<'a> {
    pub name: &'a str,
    pub shape: &'a [usize],
    pub values: Values<'a>,
}

/// An input's values, of its type.
pub(crate) enum Values<'a> {
    F32(&'a [f32]),
    I32(&'a [i32]),
}

/// `array` (which must be `expected` in shape, when given), its shape and its values as
/// contiguous `f32`s times `scale`. The Neural Engine answers in half precision and may
/// pad rows: read by the strides.
#[allow(deprecated)] // `dataPointer`: see `Model::predict`.
fn read(
    array: &MLMultiArray,
    expected: Option<&[usize]>,
    scale: f32,
) -> Result<(Vec<usize>, Vec<f32>), InferenceError> {
    let numbers = |list: Retained<NSArray<NSNumber>>| -> Vec<usize> {
        list.iter().map(|n| n.as_usize()).collect()
    };
    let shape = numbers(unsafe { array.shape() });
    if expected.is_some_and(|e| e != shape) || shape.is_empty() || shape.contains(&0) {
        return Err(InferenceError::OutputInvalid(format!(
            "the model answered with shape {shape:?}, expected {expected:?}"
        )));
    }
    let strides = numbers(unsafe { array.strides() });
    // The last value's offset, plus one: how far the strides reach.
    let len = shape
        .iter()
        .zip(&strides)
        .map(|(n, s)| (n - 1) * s)
        .sum::<usize>()
        + 1;
    let (inner, inner_stride) = (shape[shape.len() - 1], strides[strides.len() - 1]);
    let rows = shape[..shape.len() - 1].iter().product::<usize>();
    // Each row's offset: its index written out over the outer dimensions.
    let row_offset = |mut row: usize| -> usize {
        let mut offset = 0;
        for d in (0..shape.len() - 1).rev() {
            offset += (row % shape[d]) * strides[d];
            row /= shape[d];
        }
        offset
    };
    let base = unsafe { array.dataPointer() };
    let mut out = Vec::with_capacity(rows * inner);
    match unsafe { array.dataType() } {
        MLMultiArrayDataType::Float16 => {
            // SAFETY: the array holds `len` half-precision values by its own strides.
            let data =
                unsafe { std::slice::from_raw_parts(base.cast::<half::f16>().as_ptr(), len) };
            for row in 0..rows {
                let start = row_offset(row);
                out.extend((0..inner).map(|x| data[start + x * inner_stride].to_f32() * scale));
            }
        }
        MLMultiArrayDataType::Float32 => {
            // SAFETY: as above, single precision.
            let data = unsafe { std::slice::from_raw_parts(base.cast::<f32>().as_ptr(), len) };
            for row in 0..rows {
                let start = row_offset(row);
                out.extend((0..inner).map(|x| data[start + x * inner_stride] * scale));
            }
        }
        other => {
            return Err(InferenceError::OutputInvalid(format!(
                "the model answered in data type {}",
                other.0
            )));
        }
    }
    Ok((shape, out))
}

/// `load` on a thread of its own, given up after `limit`: Core ML's load cannot be
/// cancelled, so a load that never returns keeps its thread, and `stalled` is set so
/// that later calls fail at once.
fn within<T: Send + 'static>(
    limit: Duration,
    stalled: &'static AtomicBool,
    load: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if stalled.load(Ordering::Relaxed) {
        return Err("an earlier load for the Neural Engine never finished".into());
    }
    let (send, answer) = mpsc::channel();
    std::thread::Builder::new()
        .name("coreml-load".into())
        .spawn(move || {
            // The receiver is gone when the load came too late; its model is dropped.
            let _ = send.send(load());
        })
        .map_err(|e| format!("no thread for the model's load: {e}"))?;
    match answer.recv_timeout(limit) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            stalled.store(true, Ordering::Relaxed);
            Err(format!(
                "the load for the Neural Engine took over {} s",
                limit.as_secs()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the model's load ended without an answer".into())
        }
    }
}

fn message(e: &NSError) -> String {
    e.localizedDescription().to_string()
}

fn failed(e: &NSError) -> InferenceError {
    InferenceError::Failed(message(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_load_that_never_returns_fails_after_its_limit_and_later_loads_fail_at_once() {
        static STALLED: AtomicBool = AtomicBool::new(false);
        let start = std::time::Instant::now();
        let hung = within(Duration::from_millis(100), &STALLED, || {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(())
        });
        assert!(hung.is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        let start = std::time::Instant::now();
        assert!(within(Duration::from_secs(60), &STALLED, || Ok(1)).is_err());
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn a_load_within_its_limit_answers_and_its_error_passes_through() {
        static STALLED: AtomicBool = AtomicBool::new(false);
        assert_eq!(within(Duration::from_secs(5), &STALLED, || Ok(7)), Ok(7));
        assert_eq!(
            within::<()>(Duration::from_secs(5), &STALLED, || Err("bad".into())),
            Err("bad".into())
        );
        assert!(!STALLED.load(Ordering::Relaxed));
    }
}
