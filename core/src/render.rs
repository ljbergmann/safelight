//! Stage 2: open, decode and render one photo, headless, timing each step, so
//! the app can show the result and what it cost on this device.

use rawmakase_cancel::Cancel;
use rawmakase_engine::develop::PreviewRenderer;
use rawmakase_interop::edits::{EditRecord, resolve};
use rawmakase_interop::raw_defaults::DevelopDefaults;
use rawmakase_model::camera_data::Decode;
use std::path::Path;
use std::time::Instant;

/// How much of the sensor to decode.
#[derive(uniffi::Enum, Clone, Copy)]
pub enum DecodeSize {
    /// Every photosite, as Develop and export decode.
    Full,
    /// Half size (2×2 binned), as the desktop's fast path.
    Half,
}

/// Which renderer to ask for. The GPU one falls back to the CPU by itself.
#[derive(uniffi::Enum, Clone, Copy)]
pub enum Backend {
    Cpu,
    Gpu,
}

/// A rendered photo, with what each step took.
#[derive(uniffi::Record)]
pub struct Rendering {
    /// Display pixels: `width` × `height` RGBA, 8 bits, sRGB, alpha opaque.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The camera's make and model, as the decoder read them.
    pub camera: String,
    /// The decoded image's size, before the render scaled it to fit.
    pub decoded_width: u32,
    pub decoded_height: u32,
    /// Milliseconds: opening and reading metadata, resolving the default
    /// edit, decoding (demosaicing included), setting up the renderer,
    /// rendering.
    pub open_ms: f64,
    pub resolve_ms: f64,
    pub decode_ms: f64,
    pub renderer_ms: f64,
    pub render_ms: f64,
    /// What actually rendered: "GPU", or "CPU" with the GPU's fallback reason.
    pub backend: String,
}

/// Why a render failed, as text for the app to show.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum RenderError {
    #[error("{message}")]
    Failed { message: String },
    /// The model pack must be installed first: the app offers its download.
    #[error("{message}")]
    ModelNeeded {
        model: crate::ai::AiModel,
        message: String,
    },
}

impl From<anyhow::Error> for RenderError {
    fn from(e: anyhow::Error) -> Self {
        RenderError::Failed {
            message: format!("{e:#}"),
        }
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.
}

/// The stack of the thread the core's pixel work runs on. Swift calls in
/// from its own threads, whose 512 KB stacks the engine overflows (its tone
/// curves are built on the stack); the desktop runs it on Rust threads
/// (2 MB) and the command line on the main thread (8 MB), so take the larger.
pub(crate) const STACK_BYTES: usize = 8 << 20;

/// Runs `work` on a thread of its own with [`STACK_BYTES`] of stack, and
/// waits for it. A panic comes back as an error rather than ending the app.
pub(crate) fn on_core_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, RenderError> + Send + 'static,
) -> Result<T, RenderError> {
    let thread = std::thread::Builder::new()
        .name("safelight-core".into())
        .stack_size(STACK_BYTES)
        .spawn(work)
        .map_err(|e| RenderError::Failed {
            message: format!("Couldn't start the core's thread: {e}"),
        })?;
    thread.join().unwrap_or_else(|panic| {
        let why = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown panic");
        Err(RenderError::Failed {
            message: format!("The core panicked: {why}"),
        })
    })
}

/// Develops the photo at `path` with the edit a new photo starts with (no
/// saved edit, default Raw Defaults), fitted within `max_edge` pixels.
#[uniffi::export]
pub fn render_photo(
    path: String,
    max_edge: u32,
    size: DecodeSize,
    backend: Backend,
) -> Result<Rendering, RenderError> {
    on_core_thread(move || render_on_this_thread(Path::new(&path), max_edge, size, backend))
}

fn render_on_this_thread(
    path: &Path,
    max_edge: u32,
    size: DecodeSize,
    backend: Backend,
) -> Result<Rendering, RenderError> {
    let t = Instant::now();
    let raw = rawmakase_decode::photo::open(path)?;
    let open_ms = ms(t);

    // As the desktop opens a photo without a saved edit: the installed
    // profiles, and a fitted camera look for a camera without colour data.
    let t = Instant::now();
    let profiles = rawmakase_decode::camera_look::with_default(
        path,
        &raw.metadata,
        rawmakase_model::camera_profiles::installed(&raw.metadata).0,
        Cancel::NEVER,
    );
    let recipe = resolve(
        &EditRecord::default(),
        path,
        &raw.metadata,
        &profiles,
        &DevelopDefaults::default(),
    )?
    .recipe;
    recipe.validate()?;
    let resolve_ms = ms(t);

    let camera = format!("{} {}", raw.metadata.make, raw.metadata.model);
    let t = Instant::now();
    let decode = match size {
        DecodeSize::Full => Decode::full(Default::default()),
        DecodeSize::Half => Decode::Half,
    };
    let image = raw.develop(decode, Cancel::NEVER)?;
    let decode_ms = ms(t);

    let t = Instant::now();
    let mut renderer = match backend {
        Backend::Gpu => PreviewRenderer::with_gpu(),
        Backend::Cpu => PreviewRenderer::default(),
    };
    let renderer_ms = ms(t);

    let t = Instant::now();
    let out = renderer.render(&image, &recipe, max_edge, None, Cancel::NEVER)?;
    let render_ms = ms(t);

    let backend = if renderer.used_gpu() {
        "GPU".to_string()
    } else {
        match renderer.fallback_reason() {
            Some(why) => format!("CPU ({why})"),
            None => "CPU".to_string(),
        }
    };
    let rgba = out
        .rgb8()
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    Ok(Rendering {
        rgba,
        width: out.width,
        height: out.height,
        camera,
        decoded_width: image.width,
        decoded_height: image.height,
        open_ms,
        resolve_ms,
        decode_ms,
        renderer_ms,
        render_ms,
        backend,
    })
}
