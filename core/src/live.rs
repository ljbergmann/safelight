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

use crate::ai::AiSelection;
use crate::controls::{Control, Switch};
use crate::edit::{EditState, OpenEdit, Orientation};
use crate::library::Library;
use crate::render::{DecodeSize, RenderError, STACK_BYTES, on_core_thread};
use rawmakase_ai::ai_denoise::{Request as AiRequest, State as AiState};
use rawmakase_cancel::Cancel;
use rawmakase_engine::develop::gpu::{self, Display, Processor, Slot};
use rawmakase_engine::develop::quality::Output;
use rawmakase_engine::develop::{AiSource, Geometry, PreviewRenderer};
use rawmakase_model::camera_data::{CameraImage, Decode};
use rawmakase_model::model::recipe::Recipe;
use rawmakase_model::rendered::{ClipOverlay, Histogram};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, Weak};
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
    /// Point Color's Visualize Range, for this swatch.
    visualize: Option<u32>,
    /// Clipped shadows and highlights shown in blue and red.
    clipping: ClipOverlay,
    /// Before/After: Before alone or beside the edit.
    compare: crate::compare::Compare,
    /// A preset to apply, by id, once the photo is decoded.
    preset: Option<String>,
    /// Presets to render small previews of, over the edit, between frames.
    previews: VecDeque<String>,
    /// The previews' long edge, in pixels.
    preview_edge: u32,
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
    /// Between Fit and 100%: `scale` screen pixels per photo pixel (below 1),
    /// centred on a point of the developed photo as for `Actual`.
    Scaled {
        scale: f32,
        center_x: f32,
        center_y: f32,
    },
}

struct OpenRequest {
    library: Arc<Library>,
    id: i64,
    path: PathBuf,
    decode: DecodeSize,
}

/// A mask's coverage, for drawing it over the photo.
#[derive(uniffi::Record)]
pub struct MaskOverlay {
    pub width: u32,
    pub height: u32,
    /// One byte per pixel, row by row: 255 where the mask applies fully.
    pub alpha: Vec<u8>,
}

/// A frame's histogram, for drawing.
#[derive(uniffi::Record)]
pub struct HistogramData {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub shadows_clipped: bool,
    pub highlights_clipped: bool,
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
    /// The decoded photo the worker renders, shared for turning points on
    /// screen into image space (masks, Heal).
    image: Mutex<Option<Arc<CameraImage>>>,
    /// AI Denoise's request and result for the open photo.
    ai: Mutex<AiShown>,
    /// The last frame's histogram.
    histogram: Mutex<Histogram>,
    /// Preset previews rendered so far, as JPEGs, by preset id.
    previews: Mutex<std::collections::HashMap<String, Vec<u8>>>,
}

/// AI Denoise for the decoded image on screen, as the desktop's Develop keeps
/// it: the request (withdrawn when the switch goes off or the photo closes),
/// the latest result, partial or final, and what the panel says.
#[derive(Default)]
struct AiShown {
    requested: Option<(Weak<CameraImage>, Arc<AtomicBool>)>,
    source: Option<AiSource>,
    complete: bool,
    failed: bool,
    status: Option<String>,
}

impl AiShown {
    fn asked_for(&self, image: &Arc<CameraImage>) -> bool {
        self.requested
            .as_ref()
            .is_some_and(|(asked, _)| std::ptr::eq(asked.as_ptr(), Arc::as_ptr(image)))
    }

    /// Withdraws the request and forgets the result.
    fn release(&mut self) {
        if let Some((_, wanted)) = self.requested.take() {
            wanted.store(false, Ordering::Relaxed);
        }
        *self = Self::default();
    }
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
    /// The sources already offered for one spot (its index and their
    /// offsets), so Next Source moves on to new ones, as the desktop's `/`.
    tried: Mutex<(u32, Vec<[f32; 2]>)>,
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
            image: Mutex::new(None),
            ai: Mutex::new(AiShown::default()),
            histogram: Mutex::new(Histogram::EMPTY),
            previews: Mutex::default(),
        });
        let worker_shared = shared.clone();
        let worker = std::thread::Builder::new()
            .name("safelight-live".into())
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
            tried: Mutex::new((u32::MAX, Vec::new())),
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
            // Another photo's swatches are its own.
            r.visualize = None;
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

    /// Moves Color Grading wheel `region` (0 shadows, 1 midtones, 2
    /// highlights, 3 global) to the color at `x`, `y`, in wheel radii from
    /// its centre with y down; `done` ends the drag, one History step.
    pub fn set_grade_wheel(&self, region: u32, x: f32, y: f32, done: bool) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_grade_wheel(region, Some([x, y]), done));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Takes Color Grading wheel `region`'s color off, hue and saturation,
    /// as one History step: a double tap on the wheel.
    pub fn reset_grade_wheel(&self, region: u32) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_grade_wheel(region, None, true));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Shows which colours Point Color swatch `index` selects, the rest
    /// grey, as Visualize Range does; `None` shows the edit again.
    pub fn set_visualize_point_color(&self, index: Option<u32>) {
        self.shared.request(|r| r.visualize = index);
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

    /// Sets the Amount of the preset just applied (0–2, 1 = 100%); `done`
    /// ends the drag, one History step. Does nothing once another change
    /// has ended the Amount ([`EditState::preset_amount`] is `None`).
    pub fn set_preset_amount(&self, amount: f32, done: bool) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_preset_amount(amount, done));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Saves this photo's settings in `groups` (indices from
    /// [`crate::copy_groups`]) as a new preset `name` in `group`; returns
    /// its id.
    pub fn create_preset(
        &self,
        name: String,
        group: String,
        groups: Vec<u32>,
    ) -> Result<String, RenderError> {
        let edit = self.shared.edit();
        let edit = edit.as_ref().ok_or_else(no_photo)?;
        Ok(edit.create_preset(
            &rawmakase_interop::presets::user::UserPresets::default(),
            &name,
            &group,
            &groups,
        )?)
    }

    /// Lightroom's Update with Current Settings for preset `id`, from this
    /// photo's settings.
    pub fn update_preset(&self, id: String) -> Result<(), RenderError> {
        let preset = crate::transfer::find(&id)?;
        let edit = self.shared.edit();
        let edit = edit.as_ref().ok_or_else(no_photo)?;
        Ok(edit.update_preset(
            &rawmakase_interop::presets::user::UserPresets::default(),
            &preset,
        )?)
    }

    /// The open photo's snapshots, alphabetically.
    pub fn snapshots(&self) -> Vec<crate::snapshots::SnapshotInfo> {
        self.shared
            .edit()
            .as_ref()
            .and_then(|e| e.snapshots().ok())
            .unwrap_or_default()
    }

    /// Saves the edit as it is as a snapshot, named `name` or "Snapshot N";
    /// returns its id.
    pub fn add_snapshot(&self, name: Option<String>) -> Result<i64, RenderError> {
        let mut edit = self.shared.edit();
        let edit = edit.as_mut().ok_or_else(no_photo)?;
        Ok(edit.add_snapshot(name.as_deref())?)
    }

    /// Lightroom's Update with Current Settings for snapshot `id`.
    pub fn update_snapshot(&self, id: i64) -> Result<(), RenderError> {
        let mut edit = self.shared.edit();
        Ok(edit.as_mut().ok_or_else(no_photo)?.update_snapshot(id)?)
    }

    /// Renames snapshot `id`; an empty name keeps the old one.
    pub fn rename_snapshot(&self, id: i64, name: String) -> Result<(), RenderError> {
        let edit = self.shared.edit();
        Ok(edit
            .as_ref()
            .ok_or_else(no_photo)?
            .rename_snapshot(id, &name)?)
    }

    pub fn delete_snapshot(&self, id: i64) -> Result<(), RenderError> {
        let edit = self.shared.edit();
        Ok(edit.as_ref().ok_or_else(no_photo)?.delete_snapshot(id)?)
    }

    /// Applies snapshot `id` as one History step; returns what a snapshot
    /// from Lightroom set that doesn't render here.
    pub fn apply_snapshot(&self, id: i64) -> Result<Vec<String>, RenderError> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let skipped = {
            let mut edit = self.shared.edit();
            edit.as_mut()
                .ok_or_else(no_photo)?
                .apply_snapshot(id, image.as_deref())?
        };
        self.shared.request(|_| {});
        Ok(skipped)
    }

    /// Back to the photo's starting settings: one History step.
    pub fn reset(&self) {
        let changed = self.shared.edit().as_mut().is_some_and(OpenEdit::reset);
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Clears History, keeping the edit as it is.
    pub fn clear_history(&self) {
        if let Some(edit) = self.shared.edit().as_mut() {
            edit.clear_history();
        }
    }

    /// Renders small previews of presets `ids` over the edit as it is now,
    /// `edge` pixels on their long side, one at a time between frames;
    /// [`Self::preset_preview`] returns each once it is ready. Previews of
    /// an earlier edit are dropped.
    pub fn request_preset_previews(&self, ids: Vec<String>, edge: u32) {
        self.shared
            .previews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.shared.request(|r| {
            r.previews = ids.into();
            r.preview_edge = edge.clamp(32, 512);
        });
    }

    /// Preset `id`'s preview as a JPEG, once rendered.
    pub fn preset_preview(&self, id: String) -> Option<Vec<u8>> {
        self.shared
            .previews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&id)
            .cloned()
    }

    /// Shows Before alone or beside the edit, or the edit alone.
    pub fn set_compare(&self, compare: crate::compare::Compare) {
        self.shared.request(|r| r.compare = compare);
    }

    /// Copies or swaps settings between Before and the edit (copying to
    /// the edit is one History step).
    pub fn copy_before_after(&self, copy: crate::compare::BeforeAfterCopy) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.transfer(copy));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// History's Copy History Step Settings to Before.
    pub fn before_from_step(&self, applied: u32) {
        let copied = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.before_from_history(applied as usize));
        if copied {
            self.shared.request(|_| {});
        }
    }

    /// Copy Snapshot Settings to Before; returns what a snapshot from
    /// Lightroom set that doesn't render here.
    pub fn before_from_snapshot(&self, id: i64) -> Result<Vec<String>, RenderError> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let skipped = {
            let mut edit = self.shared.edit();
            edit.as_mut()
                .ok_or_else(no_photo)?
                .before_from_snapshot(id, image.as_deref())?
        };
        self.shared.request(|_| {});
        Ok(skipped)
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

    /// A new mask from two points of the view (0–1 across the photo as
    /// shown): a gradient's ends, or nothing for a brush. One History step;
    /// returns its index.
    pub fn add_mask(
        &self,
        kind: crate::masks::NewMask,
        from: Vec<f32>,
        to: Vec<f32>,
    ) -> Option<u32> {
        let (from, to) = (point(&from)?, point(&to)?);
        self.with_image(|image, edit| {
            let mut index = None;
            edit.change("New Mask", crate::masks::new_name(kind), |r| {
                index = crate::masks::new_mask(image, r, kind, from, to)
            });
            index
        })
    }

    /// Adds a part to mask `mask` (`from` and `to` as for [`Self::add_mask`]),
    /// combined with the parts before it by `operation`: one History step.
    /// Returns the part's index.
    pub fn add_mask_component(
        &self,
        mask: u32,
        kind: crate::masks::NewMask,
        operation: crate::masks::MaskOperation,
        from: Vec<f32>,
        to: Vec<f32>,
    ) -> Option<u32> {
        let (from, to) = (point(&from)?, point(&to)?);
        self.with_image(|image, edit| {
            let mut index = None;
            edit.change(operation.name(), crate::masks::new_name(kind), |r| {
                index =
                    crate::masks::add_component(image, r, mask as usize, kind, operation, from, to)
            });
            index
        })
    }

    /// The handles of a placed gradient (part `component` of mask `mask`), as
    /// view points x, y, x, y…: a linear gradient's start, end and middle, a
    /// radial one's centre and radii. Empty for other parts.
    pub fn gradient_handles(&self, mask: u32, component: u32) -> Vec<f32> {
        self.with_image(|image, edit| {
            Some(crate::masks::handles(
                image,
                edit.recipe(),
                mask as usize,
                component as usize,
            ))
        })
        .unwrap_or_default()
    }

    /// Drags handle `handle` of a placed gradient from view point `from` to
    /// `to` (each frame of a drag moves from the last); `done` ends the drag,
    /// one History step.
    pub fn drag_gradient_handle(
        &self,
        mask: u32,
        component: u32,
        handle: u32,
        from: Vec<f32>,
        to: Vec<f32>,
        done: bool,
    ) {
        let (Some(from), Some(to)) = (point(&from), point(&to)) else {
            return;
        };
        self.with_image(|image, edit| {
            edit.drag("Move Mask", done, |r| {
                crate::masks::move_handle(
                    image,
                    r,
                    mask as usize,
                    component as usize,
                    handle,
                    from,
                    to,
                )
            })
            .then_some(())
        });
    }

    /// Mask `index`'s coverage over the photo as shown (Fit, cropped), at
    /// most `max_edge` pixels: one byte of opacity per pixel, for drawing the
    /// mask overlay. Renders on the CPU, as the desktop's overlay does: call
    /// it off the main thread.
    pub fn mask_overlay(&self, index: u32, max_edge: u32) -> Option<MaskOverlay> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let recipe = self.shared.edit().as_ref()?.recipe().clone();
        on_core_thread(move || {
            let out = rawmakase_engine::develop::render(
                &image,
                &recipe.checked()?,
                max_edge,
                Cancel::NEVER,
            )?;
            let weights = rawmakase_engine::develop::masks::overlay_weights(
                &image,
                &recipe,
                index as usize,
                &out,
                None,
            );
            Ok(weights.map(|w| MaskOverlay {
                width: out.width,
                height: out.height,
                alpha: w
                    .iter()
                    .map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
                    .collect(),
            }))
        })
        .ok()
        .flatten()
    }

    /// Lightroom's Visualize Spots for the photo as shown (Fit, cropped), at
    /// most `max_edge` pixels: fine detail in white on black, one byte per
    /// pixel, where dust and blemishes stand out. `threshold` (0–1) is the
    /// slider: higher shows fainter detail. Renders on the CPU, as the
    /// desktop does: call it off the main thread.
    pub fn visualize_spots(&self, threshold: f32, max_edge: u32) -> Option<MaskOverlay> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let recipe = self.shared.edit().as_ref()?.recipe().clone();
        on_core_thread(move || {
            let out = rawmakase_engine::develop::render(
                &image,
                &recipe.checked()?,
                max_edge,
                Cancel::NEVER,
            )?;
            let rgb = rawmakase_engine::develop::retouch::visualize_spots(&out, threshold);
            Ok(MaskOverlay {
                width: out.width,
                height: out.height,
                // Grey: red, green and blue are the same.
                alpha: rgb.iter().step_by(3).copied().collect(),
            })
        })
        .ok()
    }

    /// Inverts mask `mask`, or its part `component`: one History step.
    pub fn set_mask_invert(&self, mask: u32, component: Option<u32>, invert: bool) {
        self.edit_now("Invert Mask", |r| {
            let group = r.masks.get_mut(mask as usize)?;
            match component {
                Some(c) => group.components.get_mut(c as usize)?.invert = invert,
                None => group.invert = invert,
            }
            Some(())
        });
    }

    /// Removes part `component` of mask `mask`; the last part takes the mask
    /// with it. One History step.
    pub fn delete_mask_component(&self, mask: u32, component: u32) {
        self.edit_now("Delete Mask", |r| {
            let group = r.masks.get_mut(mask as usize)?;
            if component as usize >= group.components.len() {
                return None;
            }
            group.components.remove(component as usize);
            if group.components.is_empty() {
                r.masks.remove(mask as usize);
            }
            Some(())
        });
    }

    /// Paints a stroke into brush mask `index`: one History step.
    pub fn brush(&self, index: u32, stroke: crate::masks::Stroke) {
        self.with_image(|image, edit| {
            let aspect = view_aspect(image, edit.recipe());
            let mut painted = false;
            edit.change("Brush", "", |r| {
                painted = crate::masks::add_stroke(image, r, index as usize, &stroke, aspect)
            });
            painted.then_some(())
        });
    }

    /// Removes mask `index`, or shows or hides it: one History step.
    pub fn delete_mask(&self, index: u32) {
        self.edit_now("Delete Mask", |r| {
            ((index as usize) < r.masks.len()).then(|| {
                r.masks.remove(index as usize);
            })
        });
    }

    /// Pastes the settings of the photo left last over this one's, as one
    /// History step; returns what couldn't transfer as it was, or `None`
    /// when there was nothing to paste.
    pub fn paste_previous(&self) -> Option<Vec<String>> {
        let notes = self.shared.edit().as_mut()?.paste_previous()?;
        self.shared.request(|_| {});
        Some(notes)
    }

    /// Whether Paste from Previous has a photo to take settings from.
    pub fn has_previous(&self) -> bool {
        self.shared.edit().as_ref().is_some_and(|e| {
            e.library
                .previous
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        })
    }

    /// The parametric curve as the Tone Curve's graph draws it.
    pub fn parametric_curve(&self) -> Option<crate::tone::ParametricCurve> {
        Some(crate::tone::parametric(
            &self.shared.edit().as_ref()?.recipe().effects,
        ))
    }

    /// The slider a drag up or down at input `x` (0–1) of the parametric
    /// graph moves: its region's.
    pub fn parametric_region_control(&self, x: f32) -> Option<crate::controls::Control> {
        crate::tone::region_control(&self.shared.edit().as_ref()?.recipe().effects, x)
    }

    /// Moves the parametric curve's split `index` to `t` (0–1), or with
    /// `None` back to where it starts; `done` ends the drag, one History step.
    pub fn set_parametric_split(&self, index: u32, t: Option<f32>, done: bool) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_parametric_split(index as usize, t, done));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// The Point Curve menu for the photo.
    pub fn point_curve_menu(&self) -> Option<crate::tone::PointCurveMenu> {
        Some(crate::tone::menu(self.shared.edit().as_ref()?.recipe()))
    }

    /// Gives the photo the menu's curve `index`: one History step.
    pub fn choose_point_curve(&self, index: u32) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.choose_point_curve(index as usize));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Saves the photo's point curves under `name`, for the menu here and
    /// in Lightroom's Curves folder layout.
    pub fn save_point_curve(&self, name: String) -> Result<(), RenderError> {
        let recipe = self
            .shared
            .edit()
            .as_ref()
            .map(|e| e.recipe().clone())
            .ok_or_else(|| RenderError::Failed {
                message: "No photo is open".into(),
            })?;
        crate::tone::save(&recipe, name.trim())?;
        Ok(())
    }

    /// Starts a Targeted Adjustment drag at `at` (fractions across the
    /// photo as shown): samples the photo there as each control sees it,
    /// then decides which sliders the drag moves. Takes a moment (it renders
    /// around the point): call it off the main thread, keeping the drag's
    /// travel meanwhile. False when the colour there is one the tool leaves
    /// alone (a neutral or black), or the edit changed while sampling.
    pub fn start_targeted(
        &self,
        kind: crate::targeted::TargetKind,
        at: Vec<f32>,
    ) -> Result<bool, RenderError> {
        let [u, v] = point(&at).ok_or_else(|| RenderError::Failed {
            message: "No point".into(),
        })?;
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| RenderError::Failed {
                message: "No photo is open".into(),
            })?;
        let Some((sampled, id)) = self
            .shared
            .edit()
            .as_ref()
            .map(|e| (e.recipe().clone(), e.id))
        else {
            return Ok(false);
        };
        let recipe = sampled.clone();
        let sample = on_core_thread(move || {
            Ok(rawmakase_engine::develop::quality::targeted_sample(
                &image,
                &recipe,
                u,
                v,
                Cancel::NEVER,
            )?)
        })?;
        Ok(self
            .shared
            .edit()
            .as_mut()
            .filter(|e| e.id == id)
            .is_some_and(|e| e.start_targeted(kind, &sample, &sampled)))
    }

    /// The Targeted Adjustment drag has gone `up` points up from where it
    /// started (down negative): the sliders follow; `done` ends it, one
    /// History step. False once there is no drag.
    pub fn drag_targeted(&self, up: f32, done: bool) -> bool {
        let moved = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.drag_targeted(up, done));
        if moved {
            self.shared.request(|_| {});
        }
        moved
    }

    /// Straightens the photo so the line drawn from `from` to `to` (view
    /// points over the photo as shown) is level or plumb: one History step.
    /// False for a line too short to go by.
    pub fn straighten_with_ruler(&self, from: Vec<f32>, to: Vec<f32>) -> bool {
        let (Some(from), Some(to)) = (point(&from), point(&to)) else {
            return false;
        };
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.straighten_with_ruler(from, to));
        if changed {
            self.shared.request(|_| {});
        }
        changed
    }

    /// The Crop panel's Auto: measures the angle Upright's Level would turn
    /// the photo by and sets it, one History step ("Straighten", "Auto"), as
    /// the desktop does. Takes a moment: call it off the main thread. False,
    /// with nothing changed, when the photo changed meanwhile; an error when
    /// there is no horizon or verticals to go by.
    pub fn auto_straighten(&self) -> Result<bool, RenderError> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| RenderError::Failed {
                message: "No photo is open".into(),
            })?;
        let Some((base, id)) = self
            .shared
            .edit()
            .as_ref()
            .map(|e| (e.recipe().clone(), e.id))
        else {
            return Ok(false);
        };
        let measured = base.clone();
        let angle = on_core_thread(move || {
            Ok(rawmakase_engine::develop::upright::straighten_angle(
                &image, &measured,
            ))
        })?
        .ok_or_else(|| RenderError::Failed {
            message: "Auto straighten found no horizon or verticals to level".into(),
        })?;
        {
            let mut guard = self.shared.edit();
            let Some(edit) = guard.as_mut().filter(|e| e.id == id && *e.recipe() == base) else {
                return Ok(false);
            };
            edit.change("Straighten", "Auto", |r| r.set_auto_straighten(Some(angle)));
        }
        self.shared.request(|_| {});
        Ok(true)
    }

    /// Duplicates mask `index` as the next mask, as the desktop does: one
    /// History step. Returns the copy's index; `None` with no room for it.
    pub fn duplicate_mask(&self, index: u32) -> Option<u32> {
        let copy = self.shared.edit().as_mut()?.duplicate_mask(index as usize);
        if copy.is_some() {
            self.shared.request(|_| {});
        }
        copy.map(|i| i as u32)
    }

    /// Sets mask `index`'s sliders back to a new mask's, keeping its shape
    /// and Amount, as the desktop's Reset Sliders does: one History step.
    pub fn reset_mask_sliders(&self, index: u32) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.reset_mask_sliders(index as usize));
        if changed {
            self.shared.request(|_| {});
        }
    }

    pub fn set_mask_hidden(&self, index: u32, hidden: bool) {
        self.edit_now(if hidden { "Hide Mask" } else { "Show Mask" }, |r| {
            r.masks.get_mut(index as usize).map(|m| m.hidden = hidden)
        });
    }

    /// The sliders of mask `index`'s adjustment.
    /// Mask `index`'s sliders, grouped as the Masking panel shows them.
    pub fn mask_sections(&self, index: u32) -> Vec<crate::controls::Section> {
        let edit = self.shared.edit();
        let Some(edit) = edit.as_ref() else {
            return vec![];
        };
        crate::controls::mask_sections(edit.recipe(), &edit.metadata, index, &edit.start)
    }

    pub fn mask_sliders(&self, index: u32) -> Vec<crate::controls::SliderState> {
        let edit = self.shared.edit();
        let Some(edit) = edit.as_ref() else {
            return vec![];
        };
        crate::controls::mask_sliders(edit.recipe(), &edit.metadata, index, &edit.start)
    }

    /// The Heal and Clone spots, for drawing them over the photo.
    pub fn spots(&self) -> Vec<crate::retouch::SpotInfo> {
        self.with_image(|image, edit| {
            Some(crate::retouch::spots(
                image,
                edit.recipe(),
                view_aspect(image, edit.recipe()),
            ))
        })
        .unwrap_or_default()
    }

    /// A Heal (or Clone) spot: a circle at one view point (x, y), or brushed
    /// along several (x, y, x, y…). `radius` is a fraction of the view's long
    /// edge; the source is found as the desktop finds it. One History step.
    /// Returns its index.
    pub fn add_spot(
        &self,
        points: Vec<f32>,
        radius: f32,
        clone: bool,
        feather: f32,
        opacity: f32,
    ) -> Option<u32> {
        let points: Vec<[f32; 2]> = points.as_chunks::<2>().0.to_vec();
        self.with_image(|image, edit| {
            let shown = view_aspect(image, edit.recipe());
            let mut added = None;
            edit.change(if clone { "Clone" } else { "Heal" }, "", |r| {
                added = crate::retouch::add_spot(
                    image, r, &points, radius, shown, clone, feather, opacity,
                )
            });
            added
        })
    }

    /// Drags spot `index` (or with `source`, where it copies from) from view
    /// point `from` to `to`; `done` ends the drag, one History step.
    pub fn drag_spot(&self, index: u32, from: Vec<f32>, to: Vec<f32>, source: bool, done: bool) {
        let (Some(from), Some(to)) = (point(&from), point(&to)) else {
            return;
        };
        self.with_image(|image, edit| {
            edit.drag("Move Spot", done, |r| {
                crate::retouch::move_spot(image, r, index as usize, from, to, source)
            })
            .then_some(())
        });
    }

    /// Spot `index`'s size (a fraction of the view's long edge), Feather,
    /// Opacity and Heal or Clone; `done` ends a slider's drag, one History
    /// step.
    #[allow(clippy::too_many_arguments)]
    pub fn set_spot(
        &self,
        index: u32,
        radius: f32,
        feather: f32,
        opacity: f32,
        clone: bool,
        done: bool,
    ) {
        self.with_image(|image, edit| {
            let shown = view_aspect(image, edit.recipe());
            edit.drag("Spot Settings", done, |r| {
                crate::retouch::set_spot(
                    image,
                    r,
                    index as usize,
                    shown,
                    radius,
                    feather,
                    opacity,
                    clone,
                )
            })
            .then_some(())
        });
    }

    /// The next best source for spot `index`, past those offered before.
    /// One History step; false when there is none.
    pub fn next_spot_source(&self, index: u32) -> bool {
        let mut tried = self.tried.lock().unwrap_or_else(PoisonError::into_inner);
        if tried.0 != index {
            *tried = (index, Vec::new());
        }
        self.with_image(|image, edit| {
            let mut moved = false;
            edit.change("Next Source", "", |r| {
                moved = crate::retouch::next_source(image, r, index as usize, &mut tried.1)
            });
            moved.then_some(())
        })
        .is_some()
    }

    pub fn delete_spot(&self, index: u32) {
        self.edit_now("Delete Spot", |r| {
            (index < r.retouch.len() as u32).then(|| {
                r.retouch.remove(index as usize);
            })
        });
        self.tried.lock().unwrap_or_else(PoisonError::into_inner).0 = u32::MAX;
    }

    /// The Red Eye and Pet Eye corrections.
    pub fn red_eyes(&self) -> Vec<crate::retouch::RedEyeInfo> {
        self.with_image(|image, edit| Some(crate::retouch::red_eyes(image, edit.recipe())))
            .unwrap_or_default()
    }

    /// Corrects the red (or, for `pet`, glowing) pupil within `size` (a
    /// fraction of the view's long edge) of view point `at`: one History
    /// step. Returns its index, or Lightroom's warning when no pupil is there.
    pub fn add_red_eye(&self, at: Vec<f32>, size: f32, pet: bool) -> Result<u32, RenderError> {
        let failed = |message: String| RenderError::Failed { message };
        let at = point(&at).ok_or_else(|| failed("No point".into()))?;
        let mut outcome = Err(failed("No photo is open".into()));
        self.with_image(|image, edit| {
            let shown = view_aspect(image, edit.recipe());
            let step = if pet { "Pet Eye" } else { "Red Eye" };
            let mut after = edit.recipe().clone();
            outcome = crate::retouch::add_red_eye(image, &mut after, at, size, shown, pet)
                .map_err(failed);
            outcome
                .is_ok()
                .then(|| edit.change(step, "", |r| *r = after))
        });
        outcome
    }

    /// Correction `index`'s Pupil Size and Darken; `done` ends a slider's
    /// drag, one History step.
    pub fn set_red_eye(&self, index: u32, pupil_size: f32, darken: f32, done: bool) {
        self.with_image(|_, edit| {
            edit.drag("Red Eye Settings", done, |r| {
                crate::retouch::set_red_eye(r, index as usize, pupil_size, darken)
            })
            .then_some(())
        });
    }

    /// Drags correction `index` from view point `from` to `to`; `done` ends
    /// the drag, one History step.
    pub fn drag_red_eye(&self, index: u32, from: Vec<f32>, to: Vec<f32>, done: bool) {
        let (Some(from), Some(to)) = (point(&from), point(&to)) else {
            return;
        };
        self.with_image(|image, edit| {
            edit.drag("Move Red Eye", done, |r| {
                crate::retouch::move_red_eye(image, r, index as usize, from, to)
            })
            .then_some(())
        });
    }

    /// Gives Pet Eye correction `index` its catchlight or takes it away, one
    /// History step.
    pub fn set_red_eye_catchlight(&self, index: u32, on: bool) {
        self.edit_now("Update Pet Eye Correction", |r| {
            crate::retouch::set_catchlight(r, index as usize, on).then_some(())
        });
    }

    /// Drags Pet Eye correction `index`'s catchlight to view point `to`;
    /// `done` ends the drag, one History step.
    pub fn drag_red_eye_catchlight(&self, index: u32, to: Vec<f32>, done: bool) {
        let Some(to) = point(&to) else {
            return;
        };
        self.with_image(|image, edit| {
            edit.drag("Update Pet Eye Correction", done, |r| {
                crate::retouch::move_catchlight(image, r, index as usize, to)
            })
            .then_some(())
        });
    }

    pub fn delete_red_eye(&self, index: u32) {
        self.edit_now("Delete Red Eye", |r| {
            (index < r.red_eye.len() as u32).then(|| {
                r.red_eye.remove(index as usize);
            })
        });
    }

    /// Selects the photo's subject, sky or background with its model, as a
    /// new mask: one History step. Returns its index. Running the model takes
    /// seconds: call it off the main thread. Nothing is added when another
    /// photo opened or the masks changed meanwhile, as on the desktop.
    pub fn select_mask(&self, what: AiSelection) -> Result<Option<u32>, RenderError> {
        let none_open = || RenderError::Failed {
            message: "No photo is open".into(),
        };
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(none_open)?;
        let (library, recipe, id) = {
            let edit = self.shared.edit();
            let edit = edit.as_ref().ok_or_else(none_open)?;
            (edit.library.clone(), edit.recipe().clone(), edit.id)
        };
        let masks = recipe.masks.clone();
        let generated = on_core_thread(move || crate::ai::select(&library, &image, &recipe, what))?;
        let mut edit = self.shared.edit();
        let Some(edit) = edit.as_mut().filter(|e| e.id == id) else {
            return Ok(None);
        };
        if edit.recipe().masks != masks {
            generated.discard_unless_in(edit.recipe());
            return Err(RenderError::Failed {
                message: "The masks changed while selecting; nothing was added".into(),
            });
        }
        let mut index = None;
        edit.change(&crate::ai::step_name(what), "", |r| {
            index = crate::ai::add_mask(r, what, &generated);
        });
        generated.discard_unless_in(edit.recipe());
        self.shared.request(|_| {});
        Ok(index.map(|i| i as u32))
    }

    /// Refines part `component` of mask `mask`, a Subject (or Background)
    /// selection, from taps on the photo: `taps` are x, y pairs (fractions
    /// across the photo as shown), each adding what is there or, where
    /// `adds` is false, leaving it out. Send every tap so far each time, as
    /// each run starts from all of them. One History step ("Refine Subject").
    /// Running the model takes seconds: call it off the main thread.
    pub fn refine_mask(
        &self,
        mask: u32,
        component: u32,
        taps: Vec<f32>,
        adds: Vec<bool>,
    ) -> Result<bool, RenderError> {
        let none_open = || RenderError::Failed {
            message: "No photo is open".into(),
        };
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(none_open)?;
        let (library, recipe, id) = {
            let edit = self.shared.edit();
            let edit = edit.as_ref().ok_or_else(none_open)?;
            (edit.library.clone(), edit.recipe().clone(), edit.id)
        };
        let part = recipe
            .masks
            .get(mask as usize)
            .and_then(|m| m.components.get(component as usize))
            .cloned();
        let Some(rawmakase_model::model::masks::MaskComponent {
            shape: rawmakase_model::model::masks::MaskShape::Bitmap(bitmap),
            invert,
            ..
        }) = part
        else {
            return Ok(false);
        };
        if bitmap.source.as_ref().map(|s| s.feature.as_str())
            != Some(rawmakase_model::model::masks::FEATURE_SUBJECT)
        {
            return Err(RenderError::Failed {
                message: "Only a Subject or Background selection can be refined".into(),
            });
        }
        let points: Vec<([f32; 2], bool)> = taps
            .chunks_exact(2)
            .zip(adds)
            .map(|(p, add)| {
                (
                    crate::masks::view_to_image(&image, &recipe, p[0], p[1]),
                    add,
                )
            })
            .collect();
        if points.is_empty() {
            return Ok(false);
        }
        // Leaving out alone means nothing without something to leave it out
        // of: the selection's own extent, as on the desktop.
        let bounds = points
            .iter()
            .all(|(_, add)| !add)
            .then(|| rawmakase_model::storage::mask_assets::raster_extent(&bitmap.id))
            .flatten();
        let masks = recipe.masks.clone();
        let generated =
            on_core_thread(move || crate::ai::refine(&library, &image, &recipe, &points, bounds))?;
        let mut edit = self.shared.edit();
        let Some(edit) = edit.as_mut().filter(|e| e.id == id) else {
            return Ok(false);
        };
        if edit.recipe().masks != masks {
            generated.discard_unless_in(edit.recipe());
            return Err(RenderError::Failed {
                message: "The masks changed while refining; nothing was changed".into(),
            });
        }
        let mut replaced = false;
        let name = if invert {
            "Refine Background"
        } else {
            "Refine Subject"
        };
        edit.change(name, "", |r| {
            replaced = crate::ai::replace_part(r, mask as usize, component as usize, &generated);
        });
        generated.discard_unless_in(edit.recipe());
        self.shared.request(|_| {});
        Ok(replaced)
    }

    /// The last frame's histogram: 256 counts per channel, and whether any
    /// shadows or highlights clip.
    pub fn histogram(&self) -> HistogramData {
        let h = *self
            .shared
            .histogram
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        HistogramData {
            red: h.bins[0].to_vec(),
            green: h.bins[1].to_vec(),
            blue: h.bins[2].to_vec(),
            shadows_clipped: h.clipped.shadows.iter().any(|n| *n > 0),
            highlights_clipped: h.clipped.highlights.iter().any(|n| *n > 0),
        }
    }

    /// Shows clipped shadows (blue) and highlights (red) over the photo, as
    /// the desktop's clipping warnings do.
    pub fn set_clipping(&self, shadows: bool, highlights: bool) {
        self.shared.request(|r| {
            r.clipping = ClipOverlay {
                shadows,
                highlights,
            }
        });
    }

    /// Renders the photo again: after a model was installed, say, so AI
    /// Denoise asks for its result.
    pub fn redraw(&self) {
        self.shared.request(|_| {});
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
        // The Treatment needs the photo, for the first conversion's Auto mix.
        if which == Switch::Monochrome {
            self.with_image(|image, edit| edit.set_treatment(on, image).then_some(()));
            return;
        }
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

    /// The profiles the open photo can use, as the Profile browser lists them.
    pub fn profiles(&self) -> Vec<crate::profile::ProfileInfo> {
        self.shared
            .edit()
            .as_ref()
            .map(OpenEdit::profiles)
            .unwrap_or_default()
    }

    /// Lists the open photo's profiles again, once an import has added some.
    /// The edit keeps the profile it has; only the list changes.
    pub fn reload_profiles(&self) {
        let Some((id, path, metadata)) = self
            .shared
            .edit()
            .as_ref()
            .map(|e| (e.id, e.path.clone(), e.metadata.clone()))
        else {
            return;
        };
        // On the core's thread: a fitted look reads the RAW.
        let Ok((profiles, groups)) =
            on_core_thread(move || Ok(OpenEdit::list_profiles(&path, &metadata)))
        else {
            return;
        };
        // Unless another photo opened meanwhile.
        if let Some(edit) = self.shared.edit().as_mut().filter(|e| e.id == id) {
            edit.profiles = profiles;
            edit.profile_groups = groups;
        }
    }

    /// Chooses profile `index` from [`Self::profiles`] (`None`: the camera
    /// matrix): one History step.
    pub fn set_profile(&self, index: Option<u32>) {
        if self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_profile(index))
        {
            self.shared.request(|_| {});
        }
    }

    /// The imported lens profiles that fit the photo's camera, for Lens
    /// Corrections' profile menu.
    pub fn lens_profiles(&self) -> Vec<crate::lens::LensProfileInfo> {
        let edit = self.shared.edit();
        edit.as_ref()
            .map(|e| crate::lens::profiles(e.recipe(), &e.metadata))
            .unwrap_or_default()
    }

    /// Chooses lens profile `index` (of [`Self::lens_profiles`]), or Auto
    /// with `None`: one History step.
    pub fn set_lens_profile(&self, index: Option<u32>) {
        let changed = self.shared.edit().as_mut().is_some_and(|e| {
            let mut after = e.recipe().clone();
            let Some((name, value)) = crate::lens::choose(&mut after, &e.metadata, index) else {
                return false;
            };
            if after == *e.recipe() {
                return false;
            }
            e.session.finish_gesture();
            e.session.change(
                Some(rawmakase_edit_session::history::Step::new(name, value)),
                |r| *r = after,
            );
            true
        });
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Guided Upright's guides as view points (x, y of one end, then the
    /// other, for each guide), over the photo as shown.
    pub fn upright_guides(&self) -> Vec<f32> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let (Some(image), Some(r)) = (image, self.shown_now()) else {
            return vec![];
        };
        let g = rawmakase_engine::develop::Geometry::new(&image, &r, 0);
        r.upright
            .guides
            .iter()
            .flat_map(|guide| {
                let [a, b] = [g.from_upright_frame(guide.a), g.from_upright_frame(guide.b)];
                [a[0], a[1], b[0], b[1]]
            })
            .collect()
    }

    /// Sets Guided Upright's guides from view points (as
    /// [`Self::upright_guides`] gives them, at most four), one History step
    /// named `step` (Add Guide, Move Guide, Delete Guide),
    /// analysing the photo first when it hasn't been (a moment: call it off
    /// the main thread). Returns what Guided Upright has to say (too few
    /// guides, guides it can't use fully).
    pub fn set_upright_guides(
        &self,
        points: Vec<f32>,
        step: String,
    ) -> Result<Option<String>, RenderError> {
        use rawmakase_model::model::transform::{MAX_GUIDES, UprightGuide, UprightMode};
        let failed = |message: &str| RenderError::Failed {
            message: message.into(),
        };
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| failed("The photo is still opening"))?;
        let shown = self.shown_now().ok_or_else(|| failed("No photo is open"))?;
        let g = rawmakase_engine::develop::Geometry::new(&image, &shown, 0);
        let guides: Vec<UprightGuide> = points
            .as_chunks::<4>()
            .0
            .iter()
            .take(MAX_GUIDES)
            .map(|[ax, ay, bx, by]| UprightGuide {
                a: g.upright_frame(*ax, *ay),
                b: g.upright_frame(*bx, *by),
            })
            .collect();
        // The other modes' corrections come first; Guided is solved beside them.
        if shown.upright.corrections.len() < UprightMode::Guided.code() {
            let analysed = self
                .shared
                .edit()
                .as_ref()
                .ok_or_else(|| failed("No photo is open"))?
                .recipe()
                .clone();
            let corrections = {
                let (image, analysed) = (image.clone(), analysed.clone());
                on_core_thread(move || {
                    Ok(rawmakase_engine::develop::upright::analyse(
                        &image, &analysed,
                    ))
                })?
            };
            let mut guard = self.shared.edit();
            let edit = guard.as_mut().ok_or_else(|| failed("No photo is open"))?;
            if edit.recipe().upright.corrections != analysed.upright.corrections {
                return Err(failed(
                    "The settings changed during the analysis; draw the guide again",
                ));
            }
            edit.store_upright(&analysed, &corrections);
        }
        let issue = self
            .shared
            .edit()
            .as_mut()
            .ok_or_else(|| failed("No photo is open"))?
            .set_guides(guides, &step);
        self.shared.request(|_| {});
        Ok(issue)
    }

    /// What the HDR switch shows under it, as the desktop's note: with HDR
    /// on, that the photo shows its SDR rendition (the iPad shows no HDR
    /// yet), and why HDR export can't render it, if it can't. `None` with
    /// HDR off.
    pub fn hdr_note(&self) -> Option<String> {
        let edit = self.shared.edit();
        let edit = edit.as_ref()?;
        let r = edit.recipe();
        r.hdr.is_active().then(
            || match rawmakase_model::model::hdr::check_for(r, &edit.metadata) {
                Err(e) => format!("Showing the SDR rendition. HDR export unavailable: {e}"),
                Ok(()) => "Showing the SDR rendition: HDR display isn't supported yet".to_string(),
            },
        )
    }

    /// Upright's mode, as the Transform panel shows it; `None` with no photo
    /// open.
    pub fn upright(&self) -> Option<crate::geometry_upright::Upright> {
        let mode = self.shared.edit().as_ref()?.recipe().upright.mode;
        Some(crate::geometry_upright::Upright::of(mode))
    }

    /// Chooses Upright's mode, one History step, and analyses the photo for
    /// it when it has no corrections for it yet, as the desktop does; with
    /// `again`, analyses again (Update: after changing lens corrections).
    /// The analysis takes a moment: call it off the main thread. Returns what
    /// Upright has to say (guides that correct less than they might).
    pub fn set_upright(
        &self,
        mode: crate::geometry_upright::Upright,
        again: bool,
    ) -> Result<Option<String>, RenderError> {
        let failed = |message: &str| RenderError::Failed {
            message: message.into(),
        };
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| failed("The photo is still opening"))?;
        let analysed = {
            let mut guard = self.shared.edit();
            let edit = guard.as_mut().ok_or_else(|| failed("No photo is open"))?;
            let needs = edit.set_upright(mode.mode());
            let wanted = needs || again && mode != crate::geometry_upright::Upright::Off;
            let analysed = wanted.then(|| edit.recipe().clone());
            drop(guard);
            self.shared.request(|_| {});
            match analysed {
                Some(r) => r,
                None => return Ok(None),
            }
        };
        let corrections = {
            let (image, analysed) = (image.clone(), analysed.clone());
            on_core_thread(move || {
                Ok(rawmakase_engine::develop::upright::analyse(
                    &image, &analysed,
                ))
            })?
        };
        let mut guard = self.shared.edit();
        let edit = guard.as_mut().ok_or_else(|| failed("No photo is open"))?;
        // Settings changed meanwhile (an Undo, another photo): this analysis
        // no longer fits them.
        if edit.recipe().upright.corrections != analysed.upright.corrections
            || crate::geometry_upright::inputs(edit.recipe())
                != crate::geometry_upright::inputs(&analysed)
        {
            return Err(failed(
                "The settings changed during the analysis; choose the mode again",
            ));
        }
        let issue = edit.store_upright(&analysed, &corrections);
        drop(guard);
        self.shared.request(|_| {});
        Ok(issue)
    }

    /// Profile → Fit Camera Look: fits a look to the open RAW's embedded JPEG
    /// (the camera's own rendering) and uses it, one History step. Takes a
    /// few seconds: call it off the main thread. Refused, saying why, for a
    /// photo without one, when the file or the profile changed while it was
    /// fitted (a later choice wins), as on the desktop.
    pub fn fit_camera_look(&self) -> Result<(), RenderError> {
        use rawmakase_decode::camera_look::{FitInputs, cache::FitKey, fit};
        let failed = |message: String| RenderError::Failed { message };
        let (path, before) = {
            let edit = self.shared.edit();
            let edit = edit
                .as_ref()
                .ok_or_else(|| failed("No photo is open".into()))?;
            if edit.metadata.is_rendered() {
                return Err(failed(
                    "Camera Look is fitted to a RAW's embedded preview; this photo isn't a RAW"
                        .into(),
                ));
            }
            (edit.path.clone(), edit.recipe().profile.clone())
        };
        let key = FitKey::of(&path).ok();
        let fitted = {
            let path = path.clone();
            on_core_thread(move || {
                fit(&path, &FitInputs::default(), Cancel::NEVER)
                    .map(|f| f.profile)
                    .map_err(|e| failed(format!("No camera look fitted: {}", e.reason)))
            })?
        };
        if key.is_none() || FitKey::of(&path).ok() != key {
            return Err(failed(
                "No camera look fitted: the file changed while it was fitted".into(),
            ));
        }
        let mut guard = self.shared.edit();
        let edit = guard
            .as_mut()
            .filter(|e| e.path == path)
            .ok_or_else(|| failed("Another photo opened while the look was fitted".into()))?;
        if edit.recipe().profile != before {
            return Err(failed(
                "The profile changed while the camera look was fitted; fit it again to use it"
                    .into(),
            ));
        }
        edit.apply_fitted_look(Arc::new(fitted));
        drop(guard);
        self.shared.request(|_| {});
        Ok(())
    }

    /// Point curve `channel` (0 RGB, 1 Red, 2 Green, 3 Blue) as drawn.
    pub fn point_curve(&self, channel: u32) -> Option<crate::curve::CurveState> {
        crate::curve::state(self.shared.edit().as_ref()?.recipe(), channel)
    }

    /// Lightroom's built-in point curves' names.
    pub fn builtin_curves(&self) -> Vec<String> {
        crate::curve::builtin_names()
    }

    /// Adds a point to curve `channel` at `x`, `y` (`index` none) or moves
    /// point `index` there; returns the point's index for the rest of the
    /// drag. `done` ends the drag, one History step.
    pub fn set_curve_point(
        &self,
        channel: u32,
        index: Option<u32>,
        x: f32,
        y: f32,
        done: bool,
    ) -> Option<u32> {
        let out = self
            .shared
            .edit()
            .as_mut()?
            .curve_point(channel, index, [x, y], done);
        self.shared.request(|_| {});
        out
    }

    pub fn remove_curve_point(&self, channel: u32, index: u32) {
        if self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.remove_curve_point(channel, index))
        {
            self.shared.request(|_| {});
        }
    }

    pub fn set_builtin_curve(&self, name: String) {
        if self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_builtin_curve(&name))
        {
            self.shared.request(|_| {});
        }
    }

    /// Sets white balance to As Shot or a named preset: one History step.
    pub fn set_white_balance(&self, name: String) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.set_white_balance(&name));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// The eyedropper: makes the point `at` of the view (0–1 across the
    /// photo as shown) neutral, as one History step.
    pub fn pick_white_balance(&self, at: Vec<f32>) {
        let Some([u, v]) = point(&at) else { return };
        self.with_image(|image, edit| {
            edit.change("White Balance", "Custom", |r| {
                crate::wb::pick(image, r, u, v)
            });
            Some(())
        });
    }

    /// Runs Auto Settings or Auto white balance and applies it as one
    /// History step. Takes a moment (a reduced render): call it off the main
    /// thread. Nothing changes when another photo opened, or the settings
    /// Auto measures changed, while it ran.
    pub fn auto(&self, kind: crate::wb::AutoKind) -> Result<bool, RenderError> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| RenderError::Failed {
                message: "No photo is open".into(),
            })?;
        let Some((base, id)) = self
            .shared
            .edit()
            .as_ref()
            .map(|e| (e.recipe().clone(), e.id))
        else {
            return Ok(false);
        };
        let estimate_base = base.clone();
        let auto = on_core_thread(move || Ok(crate::wb::estimate(kind, &image, &estimate_base)?))?;
        {
            let mut guard = self.shared.edit();
            let Some(edit) = guard.as_mut().filter(|e| e.id == id) else {
                return Ok(false);
            };
            if crate::wb::inputs(kind, edit.recipe()) != crate::wb::inputs(kind, &base) {
                return Ok(false);
            }
            let (name, value) = kind.step();
            edit.change(name, value, |r| crate::wb::apply(kind, &auto, r));
        }
        self.shared.request(|_| {});
        Ok(true)
    }

    /// Point Color's swatches, with their sliders.
    pub fn point_colors(&self) -> Vec<crate::point_color::SwatchInfo> {
        self.shared
            .edit()
            .as_ref()
            .map(|e| crate::point_color::swatches(e.recipe(), &e.metadata))
            .unwrap_or_default()
    }

    /// Samples the color at `at` (fractions across the photo as shown) and
    /// adds it as a Point Color swatch, one History step. Returns its index,
    /// or why it wasn't added. Takes a moment (it renders around the point):
    /// call it off the main thread. A sample of a photo edited meanwhile is
    /// dropped, as on the desktop.
    pub fn add_point_color(&self, at: Vec<f32>) -> Result<u32, RenderError> {
        let failed = |message: &str| RenderError::Failed {
            message: message.into(),
        };
        let [u, v] = point(&at).ok_or_else(|| failed("No point"))?;
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| failed("No photo is open"))?;
        let (sampled, id) = self
            .shared
            .edit()
            .as_ref()
            .map(|e| (e.recipe().clone(), e.id))
            .ok_or_else(|| failed("No photo is open"))?;
        let recipe = sampled.clone();
        let source = on_core_thread(move || {
            Ok(rawmakase_engine::develop::quality::point_color_pick(
                &image,
                &recipe,
                u,
                v,
                Cancel::NEVER,
            )?)
        })?;
        let index = {
            let mut guard = self.shared.edit();
            let edit = guard
                .as_mut()
                .filter(|e| e.id == id && *e.recipe() == sampled)
                .ok_or_else(|| {
                    failed("The photo changed while sampling; sample the color again")
                })?;
            edit.add_point_color(source).map_err(|m| failed(&m))?
        };
        self.shared.request(|_| {});
        Ok(index as u32)
    }

    /// Which point of swatch `swatch`'s range `which` a press at `t` (0–1
    /// along its rail) takes, as the desktop picks it.
    pub fn nearest_range_point(&self, swatch: u32, which: u32, t: f32) -> u32 {
        let edit = self.shared.edit();
        let points = edit
            .as_ref()
            .and_then(|e| e.recipe().point_colors.get(swatch as usize).copied())
            .zip(crate::point_color::RANGES.get(which as usize))
            .map(|(p, &w)| p.range_points(w));
        points.map_or(0, |p| {
            rawmakase_model::model::point_color::nearest_point(&p, t) as u32
        })
    }

    /// Moves a point of a Point Color swatch's range (see
    /// [`Self::nearest_range_point`]); `done` ends the drag, one History step.
    /// `None` for `point` resets the range.
    pub fn set_point_color_range(
        &self,
        swatch: u32,
        which: u32,
        point: Option<u32>,
        t: f32,
        done: bool,
    ) {
        let changed = self.shared.edit().as_mut().is_some_and(|e| {
            e.set_point_color_range(
                swatch as usize,
                which as usize,
                point.map(|k| k as usize),
                t,
                done,
            )
        });
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Deletes Point Color swatch `index`, or every swatch with `None`: one
    /// History step.
    pub fn delete_point_color(&self, index: Option<u32>) {
        let changed = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.delete_point_color(index.map(|i| i as usize)));
        if changed {
            self.shared.request(|_| {});
        }
    }

    /// Shows the edit with `applied` of History's steps applied, as tapping
    /// a step in the History list does.
    pub fn go_to_step(&self, applied: u32) {
        let moved = self
            .shared
            .edit()
            .as_mut()
            .is_some_and(|e| e.go_to(applied as usize));
        if moved {
            self.shared.request(|_| {});
        }
    }

    /// The sliders, History and save state; `None` until the edit has loaded.
    pub fn edit_state(&self) -> Option<EditState> {
        let mut state = self.shared.edit().as_ref().map(OpenEdit::state)?;
        state.ai_denoise = self
            .shared
            .ai
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .status
            .clone();
        Some(state)
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

impl LiveView {
    /// The photo's settings as the live view shows them now (whole while
    /// cropping), for mapping touches to the photo.
    fn shown_now(&self) -> Option<Recipe> {
        let cropping = self
            .shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .cropping;
        Some(shown(self.shared.edit().as_ref()?.recipe(), cropping, None))
    }

    /// Runs `work` with the decoded photo and the edit, then asks for a frame.
    fn with_image<T>(
        &self,
        work: impl FnOnce(&CameraImage, &mut OpenEdit) -> Option<T>,
    ) -> Option<T> {
        let image = self
            .shared
            .image
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let out = {
            let mut edit = self.shared.edit();
            work(&image, edit.as_mut()?)
        };
        if out.is_some() {
            self.shared.request(|_| {});
        }
        out
    }

    /// A change to the recipe as one History step, when `change` makes one.
    fn edit_now(&self, step: &str, change: impl FnOnce(&mut Recipe) -> Option<()>) {
        let changed = self.shared.edit().as_mut().is_some_and(|e| {
            let mut after = e.recipe().clone();
            change(&mut after).is_some() && {
                e.change(step, "", |r| *r = after);
                true
            }
        });
        if changed {
            self.shared.request(|_| {});
        }
    }
}

impl Drop for LiveView {
    /// Only asks the worker to stop: it holds the surface and its own
    /// reference to the layer, so it may finish on its own time.
    fn drop(&mut self) {
        // The photo left when the editor closes, for Paste from Previous.
        if let Some(edit) = self.shared.edit().as_ref() {
            edit.leave();
        }
        self.shared.request(|r| r.quit = true);
    }
}

/// The open photo's decoded image and what renders it.
struct Photo {
    id: i64,
    /// The file as it was before it was decoded: AI Denoise refuses a file
    /// changed since, as the desktop does.
    file: Option<rawmakase_model::storage::Identity>,
    image: Arc<CameraImage>,
    renderer: PreviewRenderer,
}

/// The AI Denoise result the next frame of `p` shows with `recipe`, asking
/// the service for one when the edit renders AI Denoise and none was asked
/// for this image yet; withdraws the request when it no longer does.
fn ai_denoise(shared: &Arc<Shared>, p: &Photo, recipe: &Recipe) -> Option<AiSource> {
    let mut ai = shared.ai.lock().unwrap_or_else(PoisonError::into_inner);
    if !recipe.ai_denoise.renders() {
        ai.release();
        return None;
    }
    if p.image.fast {
        return None;
    }
    if !ai.asked_for(&p.image) && !ai.complete && !ai.failed {
        let Some((library, path)) = shared
            .edit()
            .as_ref()
            .filter(|e| e.id == p.id)
            .map(|e| (e.library.clone(), e.path.clone()))
        else {
            return None;
        };
        let Some(file) = p.file.clone() else {
            ai.failed = true;
            ai.status = Some("The photo changed since it was opened".into());
            return None;
        };
        let service = library.ai.denoise();
        if !rawmakase_ai::model_install::Pack::Denoise.is_installed() {
            ai.status = Some("Install the AI Denoise model to use it.".into());
            return None;
        }
        let wanted = Arc::new(AtomicBool::new(true));
        ai.requested = Some((Arc::downgrade(&p.image), wanted.clone()));
        ai.status = Some("Waiting…".into());
        // The service may answer at once, from its cache, through the sink,
        // which takes this lock.
        drop(ai);
        let sink_shared = Arc::downgrade(shared);
        service.request(AiRequest {
            path,
            file,
            plain: p.image.clone(),
            region: None,
            wanted,
            sink: Arc::new(move |update| {
                if let Some(shared) = sink_shared.upgrade() {
                    ai_update(&shared, update);
                }
            }),
        });
        ai = shared.ai.lock().unwrap_or_else(PoisonError::into_inner);
    }
    ai.source.clone()
}

/// An update from the service: kept when it is for the image asked for, which
/// renders again with it.
fn ai_update(shared: &Shared, update: rawmakase_ai::ai_denoise::Update) {
    {
        let mut ai = shared.ai.lock().unwrap_or_else(PoisonError::into_inner);
        let current = ai
            .requested
            .as_ref()
            .is_some_and(|(asked, _)| asked.as_ptr() == update.plain.as_ptr());
        if !current {
            return;
        }
        match &update.state {
            AiState::Queued => ai.status = Some("Waiting for another photo…".into()),
            AiState::Running(done) => {
                ai.status = Some(format!("Denoising… {:.0}%", done * 100.));
            }
            AiState::Partial(source) => {
                ai.source = Some(source.clone());
                ai.status = Some("Denoising… the view is ready".into());
            }
            AiState::Ready(source) => {
                ai.source = Some(source.clone());
                ai.complete = true;
                ai.status = Some("Ready".into());
            }
            AiState::Failed(why) => {
                // A partial result isn't shown once the final one won't come.
                ai.source = None;
                ai.failed = true;
                ai.status = Some(why.clone());
            }
        }
    }
    shared.request(|_| {});
}

fn run(shared: &Arc<Shared>, layer: usize) -> anyhow::Result<()> {
    let mut screen = Screen::new(layer, current_size(shared))?;
    let mut photo: Option<Photo> = None;
    let mut times: VecDeque<f64> = VecDeque::new();
    loop {
        let (open, zoom, (cropping, visualize), preset, size, clipping, compare) = {
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
            (
                r.open.take(),
                r.zoom,
                (r.cropping, r.visualize),
                r.preset.take(),
                r.size,
                r.clipping,
                // The Crop tool shows the edit alone, as tools do on the desktop.
                if r.cropping {
                    crate::compare::Compare::Off
                } else {
                    r.compare
                },
            )
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
        let recipe = shared
            .edit()
            .as_ref()
            .map(|e| shown(e.recipe(), cropping, visualize));
        // AI Denoise runs on the full-size decode, as on the desktop: a
        // half-size photo is decoded again at full size first.
        let full_size = photo.as_ref().is_some_and(|p| p.image.fast)
            && recipe.as_ref().is_some_and(|r| r.ai_denoise.renders());
        // Taken in a statement of its own: `open_photo` locks the edit again.
        let reopen = full_size
            .then(|| {
                shared.edit().as_ref().map(|e| OpenRequest {
                    library: e.library.clone(),
                    id: e.id,
                    path: e.path.clone(),
                    decode: DecodeSize::Full,
                })
            })
            .flatten();
        if let Some(request) = reopen {
            shared.stats(|s| s.loading = true);
            open_photo(shared, &screen, &mut photo, request);
            times.clear();
            shared.stats(|s| s.loading = false);
        }
        let (Some(p), Some(recipe)) = (photo.as_mut(), recipe) else {
            screen.clear();
            continue;
        };
        let before = (compare != crate::compare::Compare::Off)
            .then(|| shared.edit().as_ref().map(OpenEdit::before_settings))
            .flatten();
        let ai = ai_denoise(shared, p, &recipe);
        let has_ai = ai.is_some();
        p.renderer.set_ai(ai);
        let shown = match (compare, &before) {
            (crate::compare::Compare::BeforeOnly, Some(before)) => {
                screen.show(p, before, None, compare, zoom, clipping, has_ai)
            }
            _ => screen.show(p, &recipe, before.as_ref(), compare, zoom, clipping, has_ai),
        };
        match shown {
            Ok(frame) => {
                *shared
                    .histogram
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = frame.histogram;
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
        // One preview between frames, so the photo stays responsive; the
        // next waits for the next turn of the loop.
        if let Some(p) = photo.as_mut() {
            render_one_preview(shared, p);
        }
    }
}

/// The edit as the view shows it: with the Crop tool open, the whole
/// straightened photo, as the desktop's Crop tool shows it; with Visualize
/// Range on, swatch `visualize`'s selection in colour and the rest grey.
/// Neither is saved.
fn shown(r: &Recipe, cropping: bool, visualize: Option<u32>) -> Recipe {
    let mut r = r.clone();
    if cropping {
        r.crop = [0., 0., 1., 1.];
        r.constrain_crop = false;
    }
    if let Some(list) = visualize.and_then(|i| {
        rawmakase_model::model::point_color::visualize_range(&r.point_colors, i as usize)
    }) {
        r.point_colors = list;
    }
    r
}

/// Preset `id` applied over `recipe`, rendered `edge` pixels on its long
/// side by `renderer`, as a JPEG: what the Presets panel shows of it.
pub(crate) fn preset_preview(
    renderer: &mut PreviewRenderer,
    image: &CameraImage,
    recipe: &Recipe,
    metadata: &rawmakase_model::camera_data::Metadata,
    profiles: &[Arc<rawmakase_model::camera_profiles::CameraProfile>],
    id: &str,
    edge: u32,
) -> anyhow::Result<Vec<u8>> {
    let preset = rawmakase_interop::presets::find_preset(id)
        .ok_or_else(|| anyhow::anyhow!("The preset is gone"))?;
    let applied = rawmakase_interop::presets::apply_to_photo(
        &preset,
        recipe,
        metadata,
        profiles,
        Some(&rawmakase_engine::develop::Measures(image)),
    )?;
    let pixels = renderer
        .render_to(image, &applied.recipe, edge, None, Cancel::NEVER, None)?
        .pixels();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80).encode(
        &pixels.rgb8(),
        pixels.width,
        pixels.height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpeg)
}

/// Renders the next preset preview asked for, if any, and asks for another
/// turn while more wait.
fn render_one_preview(shared: &Shared, p: &mut Photo) {
    let (id, edge, more) = {
        let mut r = shared
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(id) = r.previews.pop_front() else {
            return;
        };
        (id, r.preview_edge, !r.previews.is_empty())
    };
    let edit = {
        let edit = shared.edit();
        edit.as_ref()
            .map(|e| (e.recipe().clone(), e.metadata.clone(), e.profiles.clone()))
    };
    let made =
        edit.ok_or_else(|| anyhow::anyhow!("No edit"))
            .and_then(|(recipe, metadata, profiles)| {
                preset_preview(
                    &mut p.renderer,
                    &p.image,
                    &recipe,
                    &metadata,
                    &profiles,
                    &id,
                    edge,
                )
            });
    if let Ok(jpeg) = made {
        shared
            .previews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, jpeg);
    }
    if more {
        shared.request(|_| {});
    }
}

/// What a call that needs the open photo's edit says without one.
fn no_photo() -> RenderError {
    RenderError::Failed {
        message: "No photo is open".into(),
    }
}

/// Two floats as a point.
fn point(v: &[f32]) -> Option<[f32; 2]> {
    Some([*v.first()?, *v.get(1)?])
}

/// The photo as shown, cropped: its width and height over its long edge.
fn view_aspect(image: &CameraImage, r: &Recipe) -> [f32; 2] {
    let g = Geometry::new(image, r, 0);
    let long = g.width.max(g.height).max(1) as f32;
    [g.width as f32 / long, g.height as f32 / long]
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
        // The photo left, for the next one's Paste from Previous.
        if let Some(edit) = shared.edit().as_ref() {
            edit.leave();
        }
    }
    // Let go of the previous photo, its pyramid and its GPU copies before
    // decoding the next one.
    shared
        .ai
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .release();
    *photo = None;
    *shared.image.lock().unwrap_or_else(PoisonError::into_inner) = None;
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
            *shared.image.lock().unwrap_or_else(PoisonError::into_inner) = Some(p.image.clone());
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
    histogram: Histogram,
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
    /// Where each side's texture goes in its pane: the edit's, then Before's.
    fit: [wgpu::Buffer; 2],
    /// A CPU render's pixels, uploaded for drawing: the edit's, then Before's.
    upload: [Option<wgpu::Texture>; 2],
    /// Before as last rendered beside the edit, kept while what it renders
    /// stays the same, so editing the edit doesn't render it again.
    before: Option<(BeforeKey, Side)>,
}

/// What a Before render depends on.
#[derive(PartialEq)]
struct BeforeKey {
    image: *const CameraImage,
    recipe: Recipe,
    zoom: Zoom,
    size: [u32; 2],
    clipping: ClipOverlay,
    ai: bool,
}

/// A side rendered for drawing: shown one texel per pixel when `actual`,
/// else fitted to its pane.
#[derive(Clone)]
struct Side {
    texture: wgpu::Texture,
    actual: bool,
    /// For a zoomed-out view, the whole photo at its scale: the point drawn
    /// at the view's centre, as fractions of the texture.
    center: Option<[f32; 2]>,
}

/// A side to draw: its texture, where it is laid out and the part of the
/// view it shows in, in pixels.
struct Paint<'a> {
    side: &'a Side,
    pane: crate::compare::Pane,
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
                label: Some("Safelight live view"),
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
        let fit = [(); 2].map(|()| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Live view fit"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
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
            upload: [None, None],
            before: None,
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
        let file = rawmakase_model::storage::Identity::read(path).ok();
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
                file,
                image: Arc::new(image),
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
    /// Renders the edit with `recipe` and, beside it in a two-up `compare`
    /// view, Before with `before` (rendered again only when what it shows
    /// changed), then draws them.
    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        p: &mut Photo,
        recipe: &Recipe,
        before: Option<&Recipe>,
        compare: crate::compare::Compare,
        zoom: Zoom,
        clipping: ClipOverlay,
        ai: bool,
    ) -> anyhow::Result<Shown> {
        let t = Instant::now();
        let layout =
            crate::compare::panes(compare, self.config.width as f32, self.config.height as f32);
        let size = |pane: &crate::compare::Pane| {
            [pane.area[2].max(1.) as u32, pane.area[3].max(1.) as u32]
        };
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
        let (after, zoom, histogram, backend) =
            self.render(p, recipe, zoom, clipping, size(&layout.after), false)?;
        let before = match (before, layout.before) {
            (Some(recipe), Some(pane)) => {
                let key = BeforeKey {
                    image: Arc::as_ptr(&p.image),
                    recipe: recipe.clone(),
                    zoom,
                    size: size(&pane),
                    clipping,
                    ai,
                };
                let side = match self.before.take() {
                    Some((kept, side)) if kept == key => side,
                    _ => self.render(p, recipe, zoom, clipping, key.size, true)?.0,
                };
                self.before = Some((key, side.clone()));
                Some((side, pane))
            }
            _ => {
                self.before = None;
                None
            }
        };
        let render_ms = t.elapsed().as_secs_f64() * 1000.;

        let t = Instant::now();
        let (width, height) = (after.texture.width(), after.texture.height());
        let mut paints = vec![Paint {
            side: &after,
            pane: layout.after,
        }];
        if let Some((side, pane)) = &before {
            paints.push(Paint { side, pane: *pane });
        }
        self.draw(&paints);
        Ok(Shown {
            render_ms,
            present_ms: t.elapsed().as_secs_f64() * 1000.,
            backend,
            width,
            height,
            full: [g.width, g.height],
            zoom,
            frame: [uncropped.oriented_width, uncropped.oriented_height],
            histogram,
        })
    }

    /// Renders one side `size` pixels large: the whole photo fitted, or at
    /// 100% the region around `zoom`'s centre, which comes back kept inside
    /// the photo. Before renders into slots of its own, so neither side is
    /// drawn into a texture the other still shows.
    fn render(
        &mut self,
        p: &mut Photo,
        recipe: &Recipe,
        zoom: Zoom,
        clipping: ClipOverlay,
        [width, height]: [u32; 2],
        before: bool,
    ) -> anyhow::Result<(Side, Zoom, Histogram, &'static str)> {
        let g = Geometry::new(&p.image, recipe, 0);
        let mut whole = None;
        let (region, zoom) = match zoom {
            Zoom::Fit => (None, Zoom::Fit),
            // The whole photo at the scale, from the pyramid as the desktop's
            // zoomed-out views are, drawn 1:1 around the centre, kept so the
            // photo never leaves a gap where it is larger than the view.
            Zoom::Scaled {
                scale,
                center_x,
                center_y,
            } => {
                let scale = scale.clamp(0.01, 1.);
                let (tw, th) = (g.width as f32 * scale, g.height as f32 * scale);
                let keep = |c: f32, view: u32, size: f32| {
                    let half = view as f32 / 2. / size;
                    if half >= 0.5 {
                        0.5
                    } else {
                        c.clamp(half, 1. - half)
                    }
                };
                let center = [keep(center_x, width, tw), keep(center_y, height, th)];
                whole = Some((
                    (g.width.max(g.height) as f32 * scale).round().max(1.) as u32,
                    center,
                ));
                (
                    None,
                    Zoom::Scaled {
                        scale,
                        center_x: center[0],
                        center_y: center[1],
                    },
                )
            }
            Zoom::Actual { center_x, center_y } => {
                let w = width.min(g.width);
                let h = height.min(g.height);
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
            slot: match (region.is_some(), before) {
                (false, false) => Slot::Whole,
                (true, false) => Slot::Region,
                (false, true) => Slot::BeforeWhole,
                (true, true) => Slot::BeforeRegion,
            },
            clipping,
            monitor: None,
            navigator: None,
            thumbnail: None,
            samples: false,
            drawn: Vec::new(),
        };
        let max_edge = match (region, whole) {
            (Some(_), _) => 0,
            (None, Some((edge, _))) => edge,
            (None, None) => width.max(height),
        };
        let out = p.renderer.render_to(
            &p.image,
            recipe,
            max_edge,
            region,
            Cancel::NEVER,
            Some(&display),
        )?;
        let (texture, backend, histogram) = match out {
            Output::Frame(frame) => (frame.texture, "GPU", *frame.histogram),
            Output::Pixels(pixels) => {
                let histogram = pixels.histogram();
                (
                    self.upload_pixels(&pixels, before as usize),
                    "CPU",
                    histogram,
                )
            }
        };
        let side = Side {
            texture,
            actual: region.is_some() || whole.is_some(),
            center: whole.map(|(_, c)| c),
        };
        Ok((side, zoom, histogram, backend))
    }

    fn clear(&mut self) {
        self.before = None;
        self.draw(&[]);
    }

    /// Uploads a CPU render into upload texture `index` (0 the edit's, 1
    /// Before's).
    fn upload_pixels(
        &mut self,
        pixels: &rawmakase_model::rendered::Rendered,
        index: usize,
    ) -> wgpu::Texture {
        let size = wgpu::Extent3d {
            width: pixels.width,
            height: pixels.height,
            depth_or_array_layers: 1,
        };
        let reuse = self.upload[index]
            .as_ref()
            .is_some_and(|t| t.width() == size.width && t.height() == size.height);
        if !reuse {
            self.upload[index] = Some(self.device.create_texture(&wgpu::TextureDescriptor {
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
        let texture = self.upload[index].clone().expect("made above");
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

    /// Draws each side's texture centred in its pane, fitted to it or with
    /// one texel per pixel, and only within its clip, over a dark
    /// background; with no sides, only the background.
    fn draw(&mut self, paints: &[Paint<'_>]) {
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
        let (sw, sh) = (self.config.width as f32, self.config.height as f32);
        let binds: Vec<_> = paints
            .iter()
            .zip(&self.fit)
            .map(|(paint, fit)| {
                let [_, _, w, h] = paint.pane.area;
                let texture = &paint.side.texture;
                let (tw, th) = (texture.width() as f32, texture.height() as f32);
                let scale = if paint.side.actual {
                    1.
                } else {
                    (w / tw).min(h / th)
                };
                let (sx, sy) = (tw * scale / w, th * scale / h);
                // A zoomed-out view's centre point at the view's centre.
                let offset = paint.side.center.map_or([0., 0.], |[cx, cy]| {
                    [(1. - 2. * cx) * sx, (1. - 2. * cy) * sy]
                });
                let size = [sx, sy, offset[0], offset[1]];
                self.queue.write_buffer(fit, 0, bytemuck::cast_slice(&size));
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
                            resource: fit.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Live view blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            // The app's canvas (Theme.canvas), so the photo
                            // sits on the same black as the editor around it.
                            r: 0.043,
                            g: 0.043,
                            b: 0.043,
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
            pass.set_pipeline(&self.pipeline);
            for (paint, bind) in paints.iter().zip(&binds) {
                let [x, y, w, h] = paint.pane.area;
                pass.set_viewport(x, y, w, h, 0., 1.);
                // Within the surface, in whole pixels; an empty clip draws nothing.
                let [cx, cy, cw, ch] = paint.pane.clip;
                let (left, top) = (cx.clamp(0., sw) as u32, cy.clamp(0., sh) as u32);
                let right = (cx + cw).clamp(0., sw) as u32;
                let bottom = (cy + ch).clamp(0., sh) as u32;
                if right <= left || bottom <= top {
                    continue;
                }
                pass.set_scissor_rect(left, top, right - left, bottom - top);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visualize_range_shows_one_swatchs_selection_without_changing_the_edit() {
        let mut r = Recipe::default();
        crate::point_color::add(&mut r, [0.5, 0.6, 0.4]).unwrap();
        crate::point_color::add(&mut r, [3.5, 0.5, 0.3]).unwrap();
        let view = rawmakase_model::model::point_color::SwatchView::VisualizeRange;
        let s = shown(&r, false, Some(1));
        assert_eq!(s.point_colors[1].view, view);
        assert_ne!(s.point_colors[0].view, view);
        assert_eq!(s.crop, r.crop);
        // A swatch that isn't there, or none, shows the edit as it is.
        assert_eq!(shown(&r, false, Some(5)), r);
        assert_eq!(shown(&r, false, None), r);
        assert!(
            r.point_colors.iter().all(|p| p.view != view),
            "the edit itself"
        );
    }

    #[test]
    fn a_preset_preview_is_a_small_jpeg_of_the_preset_over_the_edit() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/rawmakase/tests/corpus/charts/canon-eos-r100-d65.dng");
        let raw = rawmakase_decode::photo::open(&path).unwrap();
        let profiles = rawmakase_decode::camera_look::with_default(
            &path,
            &raw.metadata,
            rawmakase_model::camera_profiles::installed(&raw.metadata).0,
            Cancel::NEVER,
        );
        let metadata = raw.metadata.clone();
        let image = raw.develop(Decode::Half, Cancel::NEVER).unwrap();
        let mut renderer = PreviewRenderer::default();
        let id = crate::transfer::presets()
            .into_iter()
            .find(|p| p.id.starts_with("builtin:"))
            .unwrap()
            .id;
        let jpeg = preset_preview(
            &mut renderer,
            &image,
            &Recipe::default(),
            &metadata,
            &profiles,
            &id,
            96,
        )
        .unwrap();
        let shown = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(shown.width().max(shown.height()), 96);
        assert!(
            preset_preview(
                &mut renderer,
                &image,
                &Recipe::default(),
                &metadata,
                &profiles,
                "gone",
                96
            )
            .is_err()
        );
    }
}
