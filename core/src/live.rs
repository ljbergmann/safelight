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
use crate::ai::AiSelection;
use crate::render::{DecodeSize, RenderError, STACK_BYTES, on_core_thread};
use rawmakase_cancel::Cancel;
use rawmakase_engine::develop::gpu::{self, Display, Processor, Slot};
use rawmakase_engine::develop::quality::Output;
use rawmakase_ai::ai_denoise::{Request as AiRequest, State as AiState};
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
    /// Clipped shadows and highlights shown in blue and red.
    clipping: ClipOverlay,
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
                index = crate::masks::add_component(image, r, mask as usize, kind, operation, from, to)
            });
            index
        })
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

    pub fn set_mask_hidden(&self, index: u32, hidden: bool) {
        self.edit_now(if hidden { "Hide Mask" } else { "Show Mask" }, |r| {
            r.masks.get_mut(index as usize).map(|m| m.hidden = hidden)
        });
    }

    /// The sliders of mask `index`'s adjustment.
    pub fn mask_sliders(&self, index: u32) -> Vec<crate::controls::SliderState> {
        let edit = self.shared.edit();
        let Some(edit) = edit.as_ref() else {
            return vec![];
        };
        crate::controls::mask_sliders(edit.recipe(), &edit.metadata, index)
    }

    /// A Heal (or Clone) spot at a view point, `radius` a fraction of the
    /// view's long edge; the source is found as the desktop finds it. One
    /// History step; false when no source fits.
    pub fn add_spot(&self, at: Vec<f32>, radius: f32, clone: bool) -> bool {
        let Some(at) = point(&at) else { return false };
        self.with_image(|image, edit| {
            let aspect = view_aspect(image, edit.recipe());
            let mut added = false;
            edit.change(if clone { "Clone" } else { "Heal" }, "", |r| {
                added = crate::masks::add_spot(image, r, at, radius, aspect, clone)
            });
            added.then_some(())
        })
        .is_some()
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
            edit.change("White Balance", "Custom", |r| crate::wb::pick(image, r, u, v));
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
        let (open, zoom, cropping, preset, size, clipping) = {
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
            (r.open.take(), r.zoom, r.cropping, r.preset.take(), r.size, r.clipping)
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
        let ai = ai_denoise(shared, p, &recipe);
        p.renderer.set_ai(ai);
        match screen.show(p, &recipe, zoom, clipping) {
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
    }
    // Let go of the previous photo, its pyramid and its GPU copies before
    // decoding the next one.
    shared.ai.lock().unwrap_or_else(PoisonError::into_inner).release();
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
    fn show(
        &mut self,
        p: &mut Photo,
        recipe: &Recipe,
        zoom: Zoom,
        clipping: ClipOverlay,
    ) -> anyhow::Result<Shown> {
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
            clipping,
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
        let (texture, backend, histogram) = match out {
            Output::Frame(frame) => (frame.texture, "GPU", *frame.histogram),
            Output::Pixels(pixels) => {
                let histogram = pixels.histogram();
                (self.upload_pixels(&pixels), "CPU", histogram)
            }
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
            histogram,
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
