//! Stage 3: a live preview drawn straight into the app's Metal layer.
//!
//! One worker thread owns everything GPU: the wgpu device, shared by the
//! engine's [`PreviewRenderer`] and a surface on the view's `CAMetalLayer`, and
//! the open photo's decoded image. The photo's edit ([`OpenEdit`]) is shared:
//! Swift changes it through this view, and the worker renders whatever it
//! holds. Swift only posts requests (open a photo, move a slider, resize) and
//! reads [`FrameStats`]. Requests are latest-only, as the
//! desktop's render worker: a render runs to the end, and the next one uses
//! whatever was asked for meanwhile, so a slider dragged faster than the GPU
//! renders skips the values in between instead of queueing them.

use crate::controls::{Control, Switch};
use crate::edit::{EditState, OpenEdit, Orientation};
use crate::library::Library;
use crate::render::{DecodeSize, RenderError, STACK_BYTES};
use rawmakase_cancel::Cancel;
use rawmakase_engine::develop::gpu::{self, Display, Processor, Slot};
use rawmakase_engine::develop::quality::Output;
use rawmakase_engine::develop::{Geometry, PreviewRenderer};
use rawmakase_model::camera_data::{CameraImage, Decode};
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::rendered::ClipOverlay;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

/// What the worker has been asked for and not yet done.
#[derive(Default)]
struct Requests {
    /// A photo of the catalog to open, replacing the current one.
    open: Option<OpenRequest>,
    zoom: Zoom,
    /// The Crop tool is open: the photo is shown uncropped.
    cropping: bool,
    /// A preset to apply, by id, once the photo is decoded.
    preset: Option<String>,
    /// The drawable's size in pixels.
    size: [u32; 2],
    /// Something changed since the last frame.
    dirty: bool,
    quit: bool,
}

/// How the photo is shown.
#[derive(uniffi::Enum, Clone, Copy, Debug, Default, PartialEq)]
pub enum Zoom {
    /// The whole photo, fitted to the view.
    #[default]
    Fit,
    /// One image pixel per screen pixel, centred on a point of the developed
    /// photo given as fractions of its width and height. With a half-size
    /// decode, a pixel of the half-size image.
    Actual { center_x: f32, center_y: f32 },
}

struct OpenRequest {
    library: Arc<Library>,
    id: i64,
    path: PathBuf,
    decode: DecodeSize,
}

/// What the last frames cost, for the app to show.
#[derive(uniffi::Record, Clone, Default)]
pub struct FrameStats {
    /// Frames presented since the view was made.
    pub frames: u64,
    /// The last frame: rendering, and drawing it into the layer.
    pub render_ms: f64,
    pub present_ms: f64,
    /// Mean render time over the last 30 frames.
    pub mean_render_ms: f64,
    /// "GPU" when the engine presented into a texture, else "CPU".
    pub backend: String,
    /// The rendered preview's size.
    pub width: u32,
    pub height: u32,
    /// The developed photo's size at the decode's full resolution: what 100%
    /// shows a pixel of.
    pub full_width: u32,
    pub full_height: u32,
    /// The straightened photo's width and height before cropping, which the
    /// crop's 0–1 coordinates span.
    pub frame_width: f32,
    pub frame_height: f32,
    /// The zoom the last frame showed, with its centre kept inside the photo.
    pub zoom: Zoom,
    /// The open photo, its camera and what opening and decoding it took.
    pub camera: String,
    pub decode_ms: f64,
    /// Opening or decoding a photo is under way.
    pub loading: bool,
    /// The last failure, if any; the view keeps the last good frame.
    pub error: Option<String>,
}

struct Shared {
    requests: Mutex<Requests>,
    wake: Condvar,
    stats: Mutex<FrameStats>,
    /// The open photo's edit, once it has loaded.
    edit: Mutex<Option<OpenEdit>>,
}

impl Shared {
    fn edit(&self) -> std::sync::MutexGuard<'_, Option<OpenEdit>> {
        self.edit.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn request(&self, change: impl FnOnce(&mut Requests)) {
        let mut r = self.requests.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut r);
        r.dirty = true;
        self.wake.notify_one();
    }

    fn stats(&self, change: impl FnOnce(&mut FrameStats)) {
        change(&mut self.stats.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// A live preview in a `CAMetalLayer`.
#[derive(uniffi::Object)]
pub struct LiveView {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

#[uniffi::export]
impl LiveView {
    /// Draws into `layer`, a `CAMetalLayer` pointer, `width` × `height` pixels.
    ///
    /// The layer must stay alive until [`Self::close`] returns; wgpu's surface
    /// retains it.
    #[uniffi::constructor]
    pub fn new(layer: u64, width: u32, height: u32) -> Result<Arc<Self>, RenderError> {
        let shared = Arc::new(Shared {
            requests: Mutex::new(Requests {
                size: [width.max(1), height.max(1)],
                ..Requests::default()
            }),
            wake: Condvar::new(),
            stats: Mutex::new(FrameStats::default()),
            edit: Mutex::new(None),
        });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("rawmakase-live".into())
            .stack_size(STACK_BYTES)
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&worker_shared, layer as usize)
                }));
                let error = match outcome {
                    Ok(Ok(())) => return,
                    Ok(Err(e)) => format!("{e:#}"),
                    Err(_) => "The live view's worker panicked".to_string(),
                };
                worker_shared.stats(|s| {
                    s.loading = false;
                    s.error = Some(error);
                });
            })
            .map_err(|e| RenderError::Failed {
                message: format!("Couldn't start the live view: {e}"),
            })?;
        Ok(Arc::new(Self {
            shared,
            worker: Mutex::new(Some(worker)),
        }))
    }

    /// Opens photo `id` of `library`, at `path`, decoded at `decode` size,
    /// with its saved edit and History; [`Self::stats`] says when it is
    /// ready. Opening the photo already open decodes it again and keeps its
    /// edit; opening another saves this one's first, and stays on it if that
    /// save fails.
    pub fn open_photo(&self, library: Arc<Library>, id: i64, path: String, decode: DecodeSize) {
        self.shared.stats(|s| s.loading = true);
        self.shared.request(|r| {
            r.open = Some(OpenRequest {
                library,
                id,
                path: PathBuf::from(path),
                decode,
            });
            r.zoom = Zoom::Fit;
        });
    }

    /// Sets a slider; `done` ends the drag, which becomes one History step.
    /// Does nothing until the photo's edit has loaded.
    pub fn set_control(&self, control: Control, value: f32, done: bool) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set(control, value, done));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Opens or closes the Crop tool: while open, the photo is shown whole,
    /// straightened, for the crop to be drawn over it.
    pub fn set_cropping(&self, on: bool) {
        self.shared.request(|r| {
            r.cropping = on;
            r.zoom = Zoom::Fit;
        });
    }

    /// Sets the crop, `[left, top, right, bottom]`; `done` ends the drag.
    pub fn set_crop(&self, crop: Vec<f32>, done: bool) {
        let Ok(crop) = <[f32; 4]>::try_from(crop) else {
            return;
        };
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_crop(crop, done));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Applies a preset (an id from [`crate::presets`]) as one History step;
    /// the worker applies it, as its Auto settings measure the decoded photo.
    pub fn apply_preset(&self, id: String) {
        self.shared.request(|r| r.preset = Some(id));
    }

    /// Copies this photo's settings in `groups` (indices from
    /// [`crate::copy_groups`]) for Paste.
    pub fn copy_settings(&self, groups: Vec<u32>) {
        if let Some(edit) = self.shared.edit().as_ref() {
            crate::transfer::copy(&edit.library, edit.recipe(), &edit.metadata, &groups);
        }
    }

    /// Pastes the copied settings over this photo's, as one History step;
    /// returns notes on what didn't transfer as it was.
    pub fn paste_settings(&self) -> Vec<String> {
        let notes = self.shared.edit().as_mut().and_then(OpenEdit::paste);
        if notes.is_some() {
            self.shared.request(|_| {});
        }
        notes.unwrap_or_default()
    }

    /// Rotates or flips the photo as shown, as one History step.
    pub fn orient(&self, orientation: Orientation) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.orient(orientation));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Turns a panel or option on or off, as one History step.
    pub fn set_switch(&self, which: Switch, on: bool) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.switch(which, on));
        if changed {
            self.shared.request(|_| {});
        }
    }

    pub fn undo(&self) {
        self.step(-1);
    }

    pub fn redo(&self) {
        self.step(1);
    }

    /// The sliders, History and save state; `None` until the edit has loaded.
    pub fn edit_state(&self) -> Option<EditState> {
        self.shared.edit().as_ref().map(OpenEdit::state)
    }

    /// Saves the edit and its History to the catalog, if anything changed:
    /// before leaving the photo, and when the app goes to the background.
    pub fn save(&self) -> Result<(), RenderError> {
        match self.shared.edit().as_mut() {
            Some(edit) => edit.save(),
            None => Ok(()),
        }
    }

    /// Fit, or 100% around a point.
    pub fn set_zoom(&self, zoom: Zoom) {
        self.shared.request(|r| r.zoom = zoom);
    }

    /// The layer's drawable changed size, in pixels.
    pub fn resize(&self, width: u32, height: u32) {
        self.shared
            .request(|r| r.size = [width.max(1), height.max(1)]);
    }

    pub fn stats(&self) -> FrameStats {
        self.shared
            .stats
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn step(&self, by: i32) {
        let moved = self.shared.edit().as_mut().is_some_and(|e| e.step(by));
        if moved {
            self.shared.request(|_| {});
        }
    }

    /// Stops the worker and waits for it: at most the render or decode it is
    /// running. Call before the layer goes away.
    pub fn close(&self) {
        self.shared.request(|r| r.quit = true);
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for LiveView {
    /// Only asks the worker to stop: it holds the surface and its own
    /// reference to the layer, so it may finish on its own time.
    fn drop(&mut self) {
        self.shared.request(|r| r.quit = true);
    }
}

/// The open photo's decoded image and what renders it.
struct Photo {
    id: i64,
    image: CameraImage,
    renderer: PreviewRenderer,
}

fn run(shared: &Shared, layer: usize) -> anyhow::Result<()> {
    let mut screen = Screen::new(layer, current_size(shared))?;
    let mut photo: Option<Photo> = None;
    let mut times: VecDeque<f64> = VecDeque::new();
    loop {
        let (open, zoom, cropping, preset, size) = {
            let mut r = shared
                .requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            while !r.dirty && !r.quit {
                r = shared.wake.wait(r).unwrap_or_else(PoisonError::into_inner);
            }
            if r.quit {
                return Ok(());
            }
            r.dirty = false;
            (r.open.take(), r.zoom, r.cropping, r.preset.take(), r.size)
        };
        screen.resize(size);

        if let Some(request) = open {
            open_photo(shared, &screen, &mut photo, request);
            times.clear();
            shared.stats(|s| s.loading = false);
        }

        if let (Some(id), Some(p)) = (preset, photo.as_ref()) {
            let applied = rawmakase_interop::presets::find_preset(&id)
                .ok_or_else(|| anyhow::anyhow!("The preset is gone"))
                .and_then(|preset| {
                    shared
                        .edit()
                        .as_mut()
                        .map_or(Ok(vec![]), |e| e.apply_preset(&preset, &p.image))
                });
            if let Err(e) = applied {
                shared.stats(|s| s.error = Some(format!("{e:#}")));
            }
        }

        // The edit as it is now; changes made while this renders ask for the
        // next frame.
        let recipe = shared.edit().as_ref().map(|e| {
            let mut r = e.recipe().clone();
            if cropping {
                // As the desktop's Crop tool: the whole straightened photo.
                r.crop = [0., 0., 1., 1.];
                r.constrain_crop = false;
            }
            r
        });
        let (Some(p), Some(recipe)) = (photo.as_mut(), recipe) else {
            screen.clear();
            continue;
        };
        match screen.show(p, &recipe, zoom) {
            Ok(frame) => {
                times.push_back(frame.render_ms);
                if times.len() > 30 {
                    times.pop_front();
                }
                let mean = times.iter().sum::<f64>() / times.len() as f64;
                shared.stats(|s| {
                    s.frames += 1;
                    s.render_ms = frame.render_ms;
                    s.present_ms = frame.present_ms;
                    s.mean_render_ms = mean;
                    s.backend = frame.backend.into();
                    s.width = frame.width;
                    s.height = frame.height;
                    s.full_width = frame.full[0];
                    s.full_height = frame.full[1];
                    s.zoom = frame.zoom;
                    s.frame_width = frame.frame[0];
                    s.frame_height = frame.frame[1];
                });
            }
            Err(e) => shared.stats(|s| s.error = Some(format!("{e:#}"))),
        }
    }
}

/// Opens `request`'s photo: saves the open edit first when it is another
/// photo's (and stays on it if that fails), lets go of the previous decode
/// before making the next, and loads the photo's edit when it isn't open yet.
fn open_photo(shared: &Shared, screen: &Screen, photo: &mut Option<Photo>, request: OpenRequest) {
    let same = shared.edit().as_ref().is_some_and(|e| e.id == request.id);
    if !same {
        let saved = shared.edit().as_mut().map_or(Ok(()), OpenEdit::save);
        if let Err(e) = saved {
            shared
                .stats(|s| s.error = Some(format!("Not opened: the edit couldn't be saved: {e}")));
            return;
        }
    }
    // Let go of the previous photo, its pyramid and its GPU copies before
    // decoding the next one.
    *photo = None;
    let opened = screen.open(&request.path, request.decode).and_then(
        |(p, metadata, profiles, decode_ms)| {
            if !same {
                let edit = OpenEdit::load(
                    request.library,
                    request.id,
                    request.path,
                    metadata.clone(),
                    &profiles,
                )?;
                *shared.edit() = Some(edit);
            }
            Ok((p, metadata, decode_ms))
        },
    );
    match opened {
        Ok((mut p, metadata, decode_ms)) => {
            p.id = request.id;
            *photo = Some(p);
            shared.stats(|s| {
                s.camera = format!("{} {}", metadata.make, metadata.model);
                s.decode_ms = decode_ms;
                s.error = None;
            });
        }
        Err(e) => shared.stats(|s| s.error = Some(format!("{e:#}"))),
    }
}

fn current_size(shared: &Shared) -> [u32; 2] {
    shared
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .size
}

struct Shown {
    render_ms: f64,
    present_ms: f64,
    backend: &'static str,
    width: u32,
    height: u32,
    full: [u32; 2],
    zoom: Zoom,
    /// The straightened photo uncropped, as `oriented_width` × `oriented_height`.
    frame: [f32; 2],
}

/// The device, the surface on the layer, and the pass that draws a preview
/// texture into it, fitted and centred.
struct Screen {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::AdapterInfo,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    fit: wgpu::Buffer,
    /// A CPU render's pixels, uploaded for drawing.
    upload: Option<wgpu::Texture>,
}

impl Screen {
    fn new(layer: usize, [width, height]: [u32; 2]) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        // SAFETY: `layer` is the view's CAMetalLayer, which the app keeps
        // alive until `LiveView::close`; wgpu retains it for the surface.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                layer as *mut std::ffi::c_void,
            ))
        }?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("RAWmakase live view"),
                // The engine's limits, within what the adapter offers: the iPad
                // simulator's Metal allows fewer inter-stage variables than
                // wgpu's defaults ask for; the M1 allows them all.
                required_limits:
                    gpu::required_limits(&adapter).or_worse_values_from(&adapter.limits()),
                ..Default::default()
            }))?;
        // The engine's textures hold display-encoded sRGB bytes: draw them into
        // a format that stores them as they are, in the layer's sRGB colour
        // space, which iPadOS maps to the display.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            // sRGB for an 8-bit format: the layer maps it to the display.
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Live view blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Live view blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Live view blit"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Live view blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Live view blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let fit = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Live view fit"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            surface,
            adapter: adapter.get_info(),
            device,
            queue,
            config,
            pipeline,
            layout,
            sampler,
            fit,
            upload: None,
        })
    }

    fn resize(&mut self, [width, height]: [u32; 2]) {
        if [self.config.width, self.config.height] == [width, height] {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        // The engine submits under this lock's read side; reconfiguring waits
        // for the GPU to go idle, which fails while another thread submits.
        let _idle = gpu::reconfiguring_surface();
        self.surface.configure(&self.device, &self.config);
    }

    /// Opens and decodes a photo, with a renderer of its own on the shared
    /// device: a new renderer drops the previous photo's pyramid and copies.
    /// Returns its metadata and the camera profiles its edit resolves with,
    /// as the desktop finds them (a fitted camera look for a camera without
    /// colour data).
    #[allow(clippy::type_complexity)]
    fn open(
        &self,
        path: &Path,
        decode: DecodeSize,
    ) -> anyhow::Result<(
        Photo,
        rawmakase_model::camera_data::Metadata,
        Vec<Arc<rawmakase_model::camera_profiles::CameraProfile>>,
        f64,
    )> {
        let t = Instant::now();
        let raw = rawmakase_decode::photo::open(path)?;
        let profiles = rawmakase_decode::camera_look::with_default(
            path,
            &raw.metadata,
            rawmakase_model::camera_profiles::installed(&raw.metadata).0,
            Cancel::NEVER,
        );
        let metadata = raw.metadata.clone();
        let image = raw.develop(
            match decode {
                DecodeSize::Full => Decode::full(Default::default()),
                DecodeSize::Half => Decode::Half,
            },
            Cancel::NEVER,
        )?;
        let decode_ms = t.elapsed().as_secs_f64() * 1000.;
        let renderer = PreviewRenderer::with_processor(Processor::with_device(
            self.device.clone(),
            self.queue.clone(),
            &self.adapter,
        ));
        Ok((
            Photo {
                id: 0,
                image,
                renderer,
            },
            metadata,
            profiles,
            decode_ms,
        ))
    }

    /// Renders the photo for the drawable's size and draws it: the whole
    /// photo from the pyramid, or at 100% the region the view covers, as the
    /// desktop's Develop does.
    fn show(&mut self, p: &mut Photo, recipe: &Recipe, zoom: Zoom) -> anyhow::Result<Shown> {
        let t = Instant::now();
        let g = Geometry::new(&p.image, recipe, 0);
        let uncropped = Geometry::new(
            &p.image,
            &Recipe {
                crop: [0., 0., 1., 1.],
                constrain_crop: false,
                ..recipe.clone()
            },
            0,
        );
        let (region, zoom) = match zoom {
            Zoom::Fit => (None, Zoom::Fit),
            Zoom::Actual { center_x, center_y } => {
                let w = self.config.width.min(g.width);
                let h = self.config.height.min(g.height);
                let x = (center_x * g.width as f32 - w as f32 / 2.)
                    .round()
                    .clamp(0., (g.width - w) as f32) as u32;
                let y = (center_y * g.height as f32 - h as f32 / 2.)
                    .round()
                    .clamp(0., (g.height - h) as f32) as u32;
                let center = Zoom::Actual {
                    center_x: (x as f32 + w as f32 / 2.) / g.width as f32,
                    center_y: (y as f32 + h as f32 / 2.) / g.height as f32,
                };
                (Some([x, y, w, h]), center)
            }
        };
        let display = Display {
            slot: if region.is_some() {
                Slot::Region
            } else {
                Slot::Whole
            },
            clipping: ClipOverlay::NONE,
            monitor: None,
            navigator: None,
            thumbnail: None,
            samples: false,
            drawn: Vec::new(),
        };
        let max_edge = if region.is_some() {
            0
        } else {
            self.config.width.max(self.config.height)
        };
        let out = p.renderer.render_to(
            &p.image,
            recipe,
            max_edge,
            region,
            Cancel::NEVER,
            Some(&display),
        )?;
        let render_ms = t.elapsed().as_secs_f64() * 1000.;

        let t = Instant::now();
        let (texture, backend) = match out {
            Output::Frame(frame) => (frame.texture, "GPU"),
            Output::Pixels(pixels) => (self.upload_pixels(&pixels), "CPU"),
        };
        let (width, height) = (texture.width(), texture.height());
        self.draw(Some((&texture, region.is_some())));
        Ok(Shown {
            render_ms,
            present_ms: t.elapsed().as_secs_f64() * 1000.,
            backend,
            width,
            height,
            full: [g.width, g.height],
            zoom,
            frame: [uncropped.oriented_width, uncropped.oriented_height],
        })
    }

    fn clear(&mut self) {
        self.draw(None);
    }

    fn upload_pixels(&mut self, pixels: &rawmakase_model::rendered::Rendered) -> wgpu::Texture {
        let size = wgpu::Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        };
        let reuse = self
            .upload
            .as_ref()
            .is_some_and(|t| t.width() == size.width && t.height() == size.height);
        if !reuse {
            self.upload = Some(self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Live view CPU render"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));
        }
        let texture = self.upload.clone().expect("made above");
        let rgba: Vec<u8> = pixels
            .rgb8()
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect();
        self.queue.write_texture(
            texture.as_image_copy(),
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size.width),
                rows_per_image: None,
            },
            size,
        );
        texture
    }

    /// Draws `texture` centred on a dark background, fitted to the surface,
    /// or with one texel per pixel when `actual`; or only the background.
    fn draw(&mut self, texture: Option<(&wgpu::Texture, bool)>) {
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            // Skip this frame; the next request draws again. An outdated or
            // lost surface is configured again first.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let _idle = gpu::reconfiguring_surface();
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = target.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let bind = texture.map(|(texture, actual)| {
            let (w, h) = (self.config.width as f32, self.config.height as f32);
            let (tw, th) = (texture.width() as f32, texture.height() as f32);
            let fit = if actual { 1. } else { (w / tw).min(h / th) };
            let scale = [tw * fit / w, th * fit / h, 0., 0.];
            self.queue
                .write_buffer(&self.fit, 0, bytemuck::cast_slice(&scale));
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Live view blit"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &texture.create_view(&Default::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.fit.as_entire_binding(),
                    },
                ],
            })
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Live view blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.11,
                            g: 0.11,
                            b: 0.12,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(bind) = &bind {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..4, 0..1);
            }
        }
        // The engine renders on this same thread, so nothing else submits
        // while the surface is reconfigured (the lock the desktop needs for its
        // UI thread).
        self.queue.submit([encoder.finish()]);
        self.queue.present(target);
    }
}
