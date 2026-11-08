//! Stage 3: a live preview drawn straight into the app's Metal layer.
//!
//! One worker thread owns everything GPU: the wgpu device, shared by the
//! engine's [`PreviewRenderer`] and a surface on the view's `CAMetalLayer`, the
//! open photo and its recipe. Swift only posts requests (open a photo, move a
//! slider, resize) and reads [`FrameStats`]. Requests are latest-only, as the
//! desktop's render worker: a render runs to the end, and the next one uses
//! whatever was asked for meanwhile, so a slider dragged faster than the GPU
//! renders skips the values in between instead of queueing them.

use crate::render::{DecodeSize, RenderError, STACK_BYTES};
use rawmakase_cancel::Cancel;
use rawmakase_engine::develop::PreviewRenderer;
use rawmakase_engine::develop::gpu::{self, Display, Processor, Slot};
use rawmakase_engine::develop::quality::Output;
use rawmakase_interop::edits::{EditRecord, resolve};
use rawmakase_interop::raw_defaults::DevelopDefaults;
use rawmakase_model::camera_data::{CameraImage, Decode};
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::rendered::ClipOverlay;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Instant;

/// What the worker has been asked for and not yet done.
#[derive(Default)]
struct Requests {
    /// A photo to open, replacing the current one.
    open: Option<(String, DecodeSize)>,
    exposure: f32,
    /// The drawable's size in pixels.
    size: [u32; 2],
    /// Something changed since the last frame.
    dirty: bool,
    quit: bool,
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
}

impl Shared {
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

    /// Opens and decodes the photo at `path` and shows it with the edit a new
    /// photo starts with; [`Self::stats`] says when it is ready.
    pub fn open(&self, path: String, size: DecodeSize) {
        self.shared.stats(|s| s.loading = true);
        self.shared.request(|r| {
            r.open = Some((path, size));
            r.exposure = 0.;
        });
    }

    /// Exposure in stops, as the Basic panel's slider.
    pub fn set_exposure(&self, stops: f32) {
        self.shared.request(|r| r.exposure = stops);
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

/// The open photo and what renders it.
struct Photo {
    image: CameraImage,
    recipe: Recipe,
    renderer: PreviewRenderer,
}

fn run(shared: &Shared, layer: usize) -> anyhow::Result<()> {
    let mut screen = Screen::new(layer, current_size(shared))?;
    let mut photo: Option<Photo> = None;
    let mut times: VecDeque<f64> = VecDeque::new();
    loop {
        let (open, exposure, size) = {
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
            (r.open.take(), r.exposure, r.size)
        };
        screen.resize(size);

        if let Some((path, decode)) = open {
            // Let go of the previous photo, its pyramid and its GPU copies
            // before decoding the next one.
            photo = None;
            times.clear();
            match screen.open(Path::new(&path), decode) {
                Ok((p, camera, decode_ms)) => {
                    photo = Some(p);
                    shared.stats(|s| {
                        s.camera = camera;
                        s.decode_ms = decode_ms;
                        s.error = None;
                    });
                }
                Err(e) => shared.stats(|s| s.error = Some(format!("{e:#}"))),
            }
            shared.stats(|s| s.loading = false);
        }

        let Some(p) = photo.as_mut() else {
            screen.clear();
            continue;
        };
        p.recipe.exposure = exposure;
        match screen.show(p) {
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
                });
            }
            Err(e) => shared.stats(|s| s.error = Some(format!("{e:#}"))),
        }
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
    fn open(&self, path: &Path, decode: DecodeSize) -> anyhow::Result<(Photo, String, f64)> {
        let t = Instant::now();
        let raw = rawmakase_decode::photo::open(path)?;
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
        let camera = format!("{} {}", raw.metadata.make, raw.metadata.model);
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
                image,
                recipe,
                renderer,
            },
            camera,
            decode_ms,
        ))
    }

    /// Renders the photo for the drawable's size and draws it.
    fn show(&mut self, p: &mut Photo) -> anyhow::Result<Shown> {
        let t = Instant::now();
        let display = Display {
            slot: Slot::Whole,
            clipping: ClipOverlay::NONE,
            monitor: None,
            navigator: None,
            thumbnail: None,
            samples: false,
            drawn: Vec::new(),
        };
        let max_edge = self.config.width.max(self.config.height);
        let out = p.renderer.render_to(
            &p.image,
            &p.recipe,
            max_edge,
            None,
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
        self.draw(Some(&texture));
        Ok(Shown {
            render_ms,
            present_ms: t.elapsed().as_secs_f64() * 1000.,
            backend,
            width,
            height,
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

    /// Draws `texture` fitted and centred on a dark background, or only the
    /// background.
    fn draw(&mut self, texture: Option<&wgpu::Texture>) {
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
        let bind = texture.map(|texture| {
            let (w, h) = (self.config.width as f32, self.config.height as f32);
            let (tw, th) = (texture.width() as f32, texture.height() as f32);
            let fit = (w / tw).min(h / th);
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
