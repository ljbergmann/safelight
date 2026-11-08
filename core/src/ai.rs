//! Stage 7 of the features: AI Denoise, and the Select Subject, Select Sky and
//! Select Background masks.
//!
//! The models are the desktop's, installed by `rawmakase-ai`'s
//! [`model_install`] into this device's data folder on the user's choice, each
//! file checked against the release's SHA-256 before it counts. ONNX Runtime
//! is Microsoft's iOS build, embedded in the app (scripts/build-ort.sh) and
//! opened by `rawmakase-inference` from the app's Frameworks folder.
//!
//! AI Denoise runs on the desktop's [`Service`]: one queue and worker for the
//! live view and export, with the results cached beside the decoded images.
//! The selections run here, on the calling thread, with the inputs and the
//! provenance the desktop records, so a mask made on the iPad reads the same
//! in RAWmakase on a computer.

use crate::library::Library;
use crate::render::RenderError;
use rawmakase_ai::ai_denoise::Service;
use rawmakase_ai::model_install::{self, InstallError, Pack, Progress};
use rawmakase_ai::selection::{self, ModelId};
use rawmakase_cancel::Cancel;
use rawmakase_inference::{Prompt, RgbImage, Session, Sky, SkySession, Subject};
use rawmakase_model::camera_data::CameraImage;
use rawmakase_model::model::masks::{
    BITMAP_SAMPLING, BitmapMask, BitmapSource, FEATURE_SKY, FEATURE_SUBJECT, MAX_GROUPS,
    MaskComponent, MaskGroup, MaskShape,
};
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::storage::bitmaps::Bitmap;
use rawmakase_model::storage::mask_assets;
use std::sync::{Arc, Mutex, PoisonError};

/// A model pack the app can install.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiModel {
    /// Select Subject and Select Background.
    Subject,
    Sky,
    Denoise,
}

impl AiModel {
    fn pack(self) -> Pack {
        match self {
            Self::Subject => Pack::Subject,
            Self::Sky => Pack::Sky,
            Self::Denoise => Pack::Denoise,
        }
    }
}

/// A model pack as the install offer shows it.
#[derive(uniffi::Record)]
pub struct AiModelInfo {
    pub model: AiModel,
    pub name: String,
    pub installed: bool,
    pub megabytes: u64,
    /// The licence and the credit, shown before the download, as on the desktop.
    pub license: String,
    pub credit: String,
}

/// Every model pack and whether it is installed.
#[uniffi::export]
pub fn ai_models() -> Vec<AiModelInfo> {
    use rawmakase_inference::{PANOPTIC, RAWNIND, SKY, SUBJECT};
    [
        (
            AiModel::Subject,
            "Select Subject",
            format!("{}\n\n{}", SUBJECT.license, PANOPTIC.license),
            format!("{}\n\n{}", SUBJECT.attribution, PANOPTIC.attribution),
        ),
        (
            AiModel::Sky,
            "Select Sky",
            SKY.license.to_string(),
            SKY.attribution.to_string(),
        ),
        (
            AiModel::Denoise,
            "AI Denoise",
            RAWNIND.license.to_string(),
            RAWNIND.attribution.to_string(),
        ),
    ]
    .into_iter()
    .map(|(model, name, license, credit)| AiModelInfo {
        model,
        name: name.into(),
        installed: model.pack().is_installed(),
        megabytes: model.pack().megabytes(),
        license,
        credit,
    })
    .collect()
}

/// A model download running on its own thread; the app polls it.
#[derive(uniffi::Object)]
pub struct ModelInstall {
    progress: Arc<Progress>,
    total: u64,
    /// `None` while running, then how it ended.
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
}

#[uniffi::export]
impl ModelInstall {
    /// Starts downloading `model`'s pack. A file already there and checked is
    /// kept, so a retry fetches only what is missing.
    #[uniffi::constructor]
    pub fn start(model: AiModel) -> Arc<Self> {
        let progress = Arc::new(Progress::default());
        let outcome = Arc::new(Mutex::new(None));
        let install = Arc::new(Self {
            progress: progress.clone(),
            total: model.pack().bytes(),
            outcome: outcome.clone(),
        });
        let spawned = std::thread::Builder::new()
            .name("model-install".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    model_install::install(model.pack(), None, &progress)
                }));
                let result = match result {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err(InstallError::from(
                        "the install stopped unexpectedly".to_string(),
                    )
                    .to_string()),
                };
                *outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
            });
        if let Err(e) = spawned {
            *install
                .outcome
                .lock()
                .unwrap_or_else(PoisonError::into_inner) =
                Some(Err(format!("Couldn't start the download: {e}")));
        }
        install
    }

    pub fn fetched_bytes(&self) -> u64 {
        self.progress.fetched()
    }

    pub fn total_bytes(&self) -> u64 {
        self.total
    }

    pub fn finished(&self) -> bool {
        self.outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Why it failed, once it has; `None` while running or after success.
    pub fn error(&self) -> Option<String> {
        self.outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|r| r.as_ref().err().cloned())
    }

    pub fn cancel(&self) {
        self.progress.cancel();
    }
}

/// The library's AI: the AI Denoise service and the selection models' loaded
/// sessions, kept while the app runs so a second selection doesn't load them
/// again.
pub(crate) struct Ai {
    pub(crate) denoise: Service,
    subject: Mutex<Option<Session>>,
    sky: Mutex<Option<SkySession>>,
}

impl Ai {
    /// With AI Denoise's results cached in `caches`' subfolder for them.
    pub(crate) fn new(caches: &std::path::Path) -> Self {
        let cache = rawmakase_decode::ai::cache::AiCache::default();
        cache.set_folder(Some(caches));
        Self {
            denoise: Service::new(cache),
            subject: Mutex::new(None),
            sky: Mutex::new(None),
        }
    }

    /// The service with the installed model, or none: a photo with AI Denoise
    /// on then fails with the reason.
    pub(crate) fn denoise(&self) -> &Service {
        let pack = Pack::Denoise;
        self.denoise
            .set_model(pack.is_installed().then(|| pack.installed_dir()));
        &self.denoise
    }
}

/// What a selection finds.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiSelection {
    Subject,
    Sky,
    /// The subject, inverted.
    Background,
}

impl AiSelection {
    fn name(self) -> &'static str {
        match self {
            Self::Subject => "Subject",
            Self::Sky => "Sky",
            Self::Background => "Background",
        }
    }

    fn model(self) -> ModelId {
        match self {
            Self::Sky => ModelId::Sky,
            Self::Subject | Self::Background => ModelId::Subject,
        }
    }

    fn pack(self) -> Pack {
        match self {
            Self::Sky => Pack::Sky,
            Self::Subject | Self::Background => Pack::Subject,
        }
    }
}

/// A selection's raster, registered in the mask store, and where it came
/// from. Saving the edit stores the raster in the catalog.
pub(crate) struct Generated {
    shape: MaskShape,
    id: String,
}

impl Generated {
    /// Forgets the raster of a selection the edit didn't take, unless an edit
    /// names it already (the same coverage selected twice has one ID).
    pub(crate) fn discard_unless_in(self, r: &Recipe) {
        if !r.mask_asset_ids().any(|i| i == self.id) {
            mask_assets::discard_unsaved(&self.id);
        }
    }
}

/// Runs the selection on `image` as `recipe` shows it, as the desktop's
/// worker does: the same input, model and keys.
pub(crate) fn select(
    library: &Library,
    image: &CameraImage,
    recipe: &Recipe,
    what: AiSelection,
) -> Result<Generated, RenderError> {
    let pack = what.pack();
    if !pack.is_installed() {
        return Err(failed(format!(
            "Install the {} model to select {}.",
            if what == AiSelection::Sky {
                "Sky"
            } else {
                "Subject"
            },
            what.name().to_lowercase()
        )));
    }
    let input = rawmakase_engine::develop::masks::selection_input(
        image,
        recipe,
        selection::INPUT_EDGE,
        Cancel::NEVER,
    )?;
    let rgb = RgbImage {
        width: input.width as usize,
        height: input.height as usize,
        data: input.rgb8(),
    };
    let model = what.model();
    let key = selection::input_key(&rgb, model);
    let dir = pack.installed_dir();
    let options = selection::load_options();
    let (coverage, prompt) = match what {
        AiSelection::Sky => {
            let mut sky = library
                .ai
                .sky
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if sky.is_none() {
                *sky = Some(Sky::load_with(&dir, &options).map_err(inference)?);
            }
            let coverage = sky
                .as_mut()
                .expect("loaded above")
                .select(&rgb, Cancel::NEVER)
                .map_err(inference)?;
            (coverage, "sky")
        }
        AiSelection::Subject | AiSelection::Background => {
            let mut subject = library
                .ai
                .subject
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if subject.is_none() {
                *subject = Some(Subject::load_with(&dir, &options).map_err(inference)?);
            }
            let analysis = subject
                .as_mut()
                .expect("loaded above")
                .analyze(&rgb, Cancel::NEVER)
                .map_err(inference)?;
            (analysis.subject().map_err(inference)?, "subject")
        }
    };
    if coverage.data.iter().all(|v| *v == 0) {
        return Err(failed(match what {
            AiSelection::Sky => "No sky was found in this photo".into(),
            _ => "No subject was found in this photo".into(),
        }));
    }
    let (width, height) = (coverage.width as u32, coverage.height as u32);
    let id = mask_assets::register(Bitmap {
        width,
        height,
        channels: 1,
        depth: 1,
        data: coverage.data,
    })?;
    let shape = MaskShape::Bitmap(BitmapMask {
        id: id.clone(),
        width,
        height,
        sampling: BITMAP_SAMPLING,
        source: Some(BitmapSource {
            feature: match what {
                AiSelection::Sky => FEATURE_SKY,
                _ => FEATURE_SUBJECT,
            }
            .into(),
            model: model.provenance(),
            input: selection::prompt_key(&key, prompt, &Prompt::default()),
        }),
    });
    Ok(Generated { shape, id })
}

/// Adds `generated` to `recipe` as a new mask named for what was selected;
/// returns its index, or `None` with no room for another mask.
pub(crate) fn add_mask(r: &mut Recipe, what: AiSelection, generated: &Generated) -> Option<usize> {
    if r.masks.len() >= MAX_GROUPS {
        return None;
    }
    let mut component = MaskComponent::new(generated.shape.clone());
    component.invert = what == AiSelection::Background;
    r.masks.push(MaskGroup {
        name: what.name().into(),
        components: vec![component],
        ..Default::default()
    });
    Some(r.masks.len() - 1)
}

/// The step name the desktop gives a selection.
pub(crate) fn step_name(what: AiSelection) -> String {
    format!("Select {}", what.name())
}

fn failed(message: String) -> RenderError {
    RenderError::Failed { message }
}

fn inference(e: rawmakase_inference::InferenceError) -> RenderError {
    failed(e.to_string())
}
