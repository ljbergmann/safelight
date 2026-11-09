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

impl Model {
    /// The model at `url` on `units`; `function`, for a model with several.
    pub(crate) fn load(
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
            read(&answer, expected, scale)
        })
    }
}

/// `array`, which must be `expected` in shape, as contiguous `f32`s times `scale`. The
/// Neural Engine answers in half precision and may pad rows: read by the strides.
#[allow(deprecated)] // `dataPointer`: see `Model::predict`.
fn read(array: &MLMultiArray, expected: &[usize], scale: f32) -> Result<Vec<f32>, InferenceError> {
    let numbers = |list: Retained<NSArray<NSNumber>>| -> Vec<usize> {
        list.iter().map(|n| n.as_usize()).collect()
    };
    let shape = numbers(unsafe { array.shape() });
    if shape != expected || shape.is_empty() || shape.contains(&0) {
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
    Ok(out)
}

fn message(e: &NSError) -> String {
    e.localizedDescription().to_string()
}

fn failed(e: &NSError) -> InferenceError {
    InferenceError::Failed(message(e))
}
