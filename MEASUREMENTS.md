# Measurements

## Stage 2: decode and render one RAW (2026-11-08)

Device: iPad Pro 12.9-inch (5th generation, M1, iPad13,8), Release build,
`-benchmark` launch option. Fitted render within 2048 px, the edit a new photo
starts with, no installed camera or lens profiles. Run one after another in the
order below, so each "before" is what the previous render left.

| Photo | Mode | Decode | Render | Total | Footprint after | Peak so far | Available after |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Canon EOS R5 CR3, 8191×5463 | GPU, full size | 1.2 s | 1.7 s | 3.1 s | 3.4 GB | 4.4 GB | 1.7 GB |
| | CPU, full size | 1.1 s | 1.0 s | 2.1 s | 3.7 GB | 4.4 GB | 1.4 GB |
| | GPU, half size | 0.5 s | 0.2 s | 0.7 s | 0.9 GB | 4.4 GB | 4.2 GB |
| Leica Q2 DNG, 8392×5632 | GPU, full size | 1.2 s | 1.9 s | 3.1 s | 3.1 GB | 4.8 GB | 2.0 GB |
| | CPU, full size | 1.1 s | 1.7 s | 2.8 s | 3.1 GB | 4.8 GB | 2.0 GB |
| | GPU, half size | 0.3 s | 0.3 s | 0.7 s | 1.6 GB | 4.8 GB | 3.5 GB |

Opening a photo takes 6–30 ms. The GPU's first render includes creating the
device (0.2 s).

- **Pixels.** The iPad's GPU render is within 1 of 255 of the Mac's CPU and GPU
  renders of the same photo (mean difference 0.0001), on both cameras. Compared
  with the Mac rendering with `HOME` pointing at an empty folder; with the
  Mac's installed profiles the renders differ, as they should.
- **The app's memory limit** on this iPad is about 5.1 GB (footprint plus
  `os_proc_available_memory`). A full-size decode peaked at 4.8 GB, within
  about 300 MB of the limit.
- **Memory comes back late, not never.** On the Mac, a full-size render
  leaves the footprint at about 5 GB for a few seconds after it returns, then
  it falls to about 650 MB with nothing else running. Renders in quick
  succession therefore stack one photo's leftovers on the next one's decode.
  Not yet known whether this is the allocator returning pages lazily or a
  thread that frees late.
- **GPU in the simulator:** falls back to the CPU, as the engine asks for 16
  inter-stage shader variables and the simulator's Metal allows 15. The M1
  allows it; the simulator's GPU results don't count.
- **Unsupported:** a Lightroom-made panorama DNG (JPEG XL–compressed RGB
  tiles) is refused with RAWmakase's own message, as on the desktop.

### What this means for the next stages

- Develop at full size only where it's needed (100% view, export); show Fit
  from a half-size decode or a smart preview. Half size uses a third of the
  memory and a quarter of the time.
- Before editing several photos in a row, find what holds memory for seconds
  after a render, and keep one photo's full-size decode alive at a time.
- Ask for Apple's increased-memory-limit entitlement, and size caches from
  `os_proc_available_memory`, not from physical RAM.
- A one-off render isn't faster on the GPU, as it includes building the
  pyramid and uploading it; the GPU's gain is in re-rendering while a slider
  moves (Stage 3).

## Stage 3: live Exposure in a Metal layer (2026-11-08)

Same iPad, Release build, `-open <photo> -sweep [-full]`: Exposure set 240
times over 4 s (−2 → +2 → −2, one value every 16 ms), the preview rendered by
the engine on the GPU into its own texture and drawn into the view's
`CAMetalLayer` at the layer's size (Fit, about 2028 px on the long edge).

| Photo | Decode | Frames in 4 s | Mean render | Draw | Footprint while editing | Peak (decode) |
| --- | --- | --- | --- | --- | --- | --- |
| Leica Q2 | half (0.35 s) | 241, 60 fps | 14 ms | 0.5 ms | 0.7 GB | 1.5 GB |
| Leica Q2 | full (1.1 s) | 236, 59 fps | 14 ms | 0.6 ms | 2.1 GB | 4.6 GB |
| Canon EOS R5 | half (0.54 s) | 241, 60 fps | 13 ms | 0.5 ms | 0.7 GB | 1.4 GB |
| Canon EOS R5 | full (1.2 s) | 236, 59 fps | 13 ms | 0.4 ms | 2.0 GB | 4.5 GB |

- Every slider value got its own frame: the sweep sends 60 values a second,
  so 60 fps is the sweep's rate. At 13–14 ms a render the core could keep up
  with about 70 a second, below the display's 120 Hz. Fit renders from the
  pyramid level nearest the view's size, so a full-size decode costs memory
  and decode time, not frame time.
- Memory while editing settles at 0.7 GB (half) or 2 GB (full); the peak is
  the decode itself, as in Stage 2.
- In the simulator the device is created with the engine's limits clamped to
  the adapter's, and the GPU path works there too (9 ms renders, on the Mac).
- Not measured yet: the latency from finger to frame.

## Stage 3: 100% and panning (2026-11-08)

Same iPad, `-open <photo> -pan [-full]`: at 100%, the view's centre moved
from 30% to 70% of the photo's width and back, 240 positions in 4 s. Each
frame renders the region the view covers (2028×1497 pixels) at full
resolution, as the desktop's 100% view does, with no reduced draft first.

| Photo | Decode | First 100% frame | Frames in 4 s | Mean render | Footprint | Peak |
| --- | --- | --- | --- | --- | --- | --- |
| Leica Q2 | half, 4184×2792 | 0.18 s | 179, 45 fps | 22 ms | 1.3 GB | 2.2 GB |
| Leica Q2 | full, 8368×5584 | 0.22 s | 179, 45 fps | 22 ms | 3.1 GB | 5.2 GB |
| Canon EOS R5 | half, 2732×4096 | 0.06 s | 203, 51 fps | 17 ms | 1.3 GB | 2.0 GB |
| Canon EOS R5 | full, 5463×8191 | 0.7–2.2 s | 182–183, 46 fps | 22 ms | 3.0 GB | 5.0 GB |

- **Without the increased-memory-limit entitlement**, the Leica at full size
  was killed by iPadOS (signal 9) as soon as it went to 100%: the full-size
  decode plus the photo kept on the GPU for regions passed the 5.1 GB limit.
  The Canon peaked at 5.0 GB, just under it. With the entitlement
  (`com.apple.developer.kernel.increased-memory-limit`, now in the app) the
  limit on this 8 GB iPad is about 8.2 GB and both run.
- The first 100% frame of a full-size decode varies (0.7 s and 2.2 s for the
  same Canon file in two runs): it uploads the full photo to the GPU.
- Panning keeps up with about three of four positions: a region costs 17–22
  ms, against 13–14 ms for Fit. The desktop draws a reduced region first
  while sliders move at 100%; panning here could do the same.
- Pinch: the frame on screen is scaled while the fingers move, then the
  view settles on 100% around the pinch point, or back to Fit. Double tap
  switches between them. Tried in the simulator, not yet by hand on the iPad.

## Freed memory that iPadOS kept counting (2026-11-08)

Apple's malloc keeps large blocks it has freed for reuse, and the footprint
iPadOS ends apps by still counts them. On the M1 iPad, a 540 MB block
allocated, filled and freed left the footprint at 553 MB; 2000 freed blocks
of 256 KB left 1055 MB (`-allocprobe`). On the Mac, with the cache turned
off (`MallocLargeCache=0`), closing a full-size Leica Q2 left 10 MB instead
of 2.5 GB: RAWmakase frees everything, the allocator kept it.

Since then the core maps blocks of 16 MB or more directly and unmaps them
when freed (`core/src/alloc.rs`). The same runs on the iPad:

| Run | Peak before | Peak now | Footprint while editing | Frame rate |
| --- | --- | --- | --- | --- |
| Leica Q2, half, Exposure sweep | 1.53 GB | 0.87 GB | 0.69 GB | 60 fps |
| Leica Q2, full, Exposure sweep | 4.62 GB | 3.24 GB | 2.06 GB | 60 fps |
| Leica Q2, full, pan at 100% | 5.18 GB | 3.31 GB | 3.07 GB | 45 fps |
| Canon EOS R5, full, Exposure sweep | 4.49 GB | 3.17 GB | 1.94 GB | 60 fps |
| Canon EOS R5, full, pan at 100% | 5.00 GB | 3.24 GB | 2.92 GB | 46 fps |
| Canon EOS R5, half, pan at 100% | 2.01 GB | 1.48 GB | 1.27 GB | 51 fps |

- Every peak is now below the default 5.1 GB limit; the increased-memory
  entitlement stays as headroom.
- The first 100% frame of a full-size decode now takes 0.15 s (it took
  0.2–2.2 s): the GPU upload no longer waits for memory held by the cache.
- What remains while editing is the photo's working set: the decoded image
  (540 MB at full size), its highlight-recovered copy, the pyramid the
  preview renders from and the copy on the GPU. Making that smaller means
  changing the engine (half-precision pixels) or editing a smart preview.

## Stage 4: a catalog in the app's storage (2026-11-08)

Same iPad. 50 Canon EOS R5 CR3s (1.9 GB) copied into the app's Documents
folder from the Mac (`devicectl`, 44 s), plus the three earlier test files.

| Step | Result |
| --- | --- |
| `-import-documents`: move 53 files into `Photos/2026-11-08/`, add them to the catalog | 53 added, 10 ms in the catalog |
| `-thumbnails`: all 53 thumbnails (340 px, from the smallest embedded preview that reaches it, 4 at a time) | 4.5 s, peak 1.27 GB |
| Same after quitting and relaunching (from the cache) | 0.09 s, peak 47 MB |
| `-rate 407A5994.CR3 3`, quit, relaunch | 53 photos, 407A5994.CR3 still 3 stars |
| Open a catalog photo in the editor (`-open 407A5994.CR3 -sweep`) | 46 fps, 20 ms renders at 1780×2668 (the editor now fills the screen) |

- The catalog is the desktop's own `.rawmakase` format in
  `Library/Application Support/RAWmakase/Catalog/`.
- In the simulator the grid, opening a photo, rating it in the editor and a
  relaunch were tried by hand (injected taps). The Files picker import was
  not tried by hand anywhere yet.
- Capture times aren't read at import (the desktop fills them in the
  background, in `catalog_session`), so the grid is in file-name order.
