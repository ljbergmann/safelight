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

## Stage 5: editing with History, saved to the catalog (2026-11-08)

The editor shows six Basic sliders (Exposure, Contrast, Highlights, Shadows,
Temp, Tint) as RAWmakase's model describes them (`ParameterId::descriptor`:
label, range, drag step, value text), changes them through
`model::edit::change_setting` and keeps the edit in RAWmakase's own
`EditSession`, moved into `crates/rawmakase-edit-session` on the fork for
this. A drag is one History step, named as the desktop names it.

| Check | Where | Result |
| --- | --- | --- |
| Drag Exposure, go to the home screen, kill the app, relaunch | simulator, by hand | Exposure +1.60 and its History step back, "Saved" |
| `-open 407A5994.CR3 -set-exposure 0.75 -save`, kill, relaunch | M1 iPad Pro | `EDIT exposure +0.75; steps ["Exposure +0.75"]; applied 1; unsaved false` |
| The iPad's catalog on the Mac (its photo folder relinked with `relink_root`), `rawmakase render --catalog` | Mac, desktop command line | byte-for-byte the same PNG as `rawmakase render --exposure 0.75`; different from the unedited render |
| Core tests (`cargo test` in `core/`) | Mac | 14 pass: a drag is one named step and survives reopening; a drag a save finishes keeps its name; an Undo is saved with its Redo; values are clamped to the slider; Temp moves the white balance; a failed save keeps the edit unsaved and says why; photos are found after the container moves |
| The fork's `make check`, with the edit session moved | Mac | passes (run with an empty `HOME`; with this Mac's own data two tests fail on `main` too, as they read installed lens profiles and AI Denoise's model) |

- **Saving:** leaving the editor saves first, and stays with an alert if
  the save fails. Resigning active and entering the background both save:
  SwiftUI's scene phase doesn't reach a view inside the navigation stack, so
  the app listens to UIKit's notifications.
- **Found by the simulator:** reinstalling the app moved its container, and
  the catalog's photo folder pointed at the old path ("No such file or
  directory"). The core now relinks a root that was an earlier container's
  photo folder, as the desktop's Find Missing Folder does.
- **Found by the simulator:** the app went to the background before the
  slider reported the end of the drag; the save finished the drag, unnamed
  ("Edit"). Steps are now named on every frame of a drag.

## Export (2026-11-08)

M1 iPad Pro, `-export <file>`: a full-size JPEG at quality 90 through
RAWmakase's export job (full-size decode, CPU reference render, metadata,
sRGB profile).

| Photo | Time | File | Peak memory | After |
| --- | --- | --- | --- | --- |
| Canon EOS R5 CR3, 8192×5464 | 11.9 s | 16.8 MB | 3.55 GB | 0.72 GB |
| Leica Q2 DNG, 8368×5584 | 9.0 s | 12.0 MB | 3.26 GB | 0.09 GB |

## AI Denoise and Select Subject, Sky and Background (2026-11-08)

M1 iPad Pro, Release build, `-open <photo> -ai-test`: installs any missing
model pack, selects the subject and the sky as masks, then turns AI Denoise on
and waits for its result. ONNX Runtime 1.23.0 (Microsoft's iOS build,
embedded), on the CPU. The photo is a Canon EOS R5 CR3 (8192×5464), indoors
under a glass roof.

| Step | Result |
| --- | --- |
| Download and check Select Subject's, Select Sky's and AI Denoise's packs | 15 s, 9 s, 3 s |
| Select Subject | 3.4–4.3 s (the first loads the models), peak 3.2 GB; one mask, "Subject" |
| Select Sky | 6.9–7.2 s; "No sky was found in this photo", which is right for it |
| AI Denoise on: decode again at full size, then the model over the whole photo | Ready in 38 s; peak 4.1 GB, 3.6 GB after |
| History | `Select Subject`, `AI Denoise On` |

On the Mac, the core's opt-in test exports a photo with AI Denoise on
through the library's service and finds its result cached
(`cargo test -- --ignored`, with `RAWMAKASE_ORT_LIB` naming the desktop's
runtime).

- **Found on the iPad:** turning AI Denoise on with a half-size decode
  deadlocked the live view's worker: the edit's lock, taken to ask for the
  full-size decode, was still held when opening the photo took it again.
- **Found by the Mac test:** AI Denoise's cache folder lives inside the
  decode cache's, which nothing had created on a new install, so results were
  computed but not kept.

## Presets made here, Preset Amount and Snapshots (2026-11-08)

M1 iPad Pro, Release build, `-open 407A5994.CR3 -develop-test`, which now
also applies built-in presets until one offers an Amount, drags it, makes a
preset of Exposure, updates, renames and deletes it, and makes, applies and
deletes a snapshot.

| Check | Result |
| --- | --- |
| Preset Amount | `Warm: 0.4, last step Preset Amount 40`: a drag through 50% to 40% is one step |
| Create, Update, Rename, Delete | listed as `Safelight Test/Develop Test`, managed; renamed; none left |
| Snapshot | applied as `Snapshot: Develop Test`, taking Exposure back from +1.50; none left |

The core's tests cover the rest on the Mac: a preset made here sets only its
groups on another photo, Update keeps its identity and groups, a second
preset of the same name and built-in presets are refused; Amount 0 is the
photo as before, 100% is the preset again with no drift, and any other edit
ends it; snapshots are this photo's, survive reopening, and an emptied name
keeps the old one.

## Before/After (2026-11-08)

Same iPad and photo, `-develop-test`: each view opened, then Exposure moved
beside it. Before renders in its own slots of the same renderer, as on the
desktop, and is kept while what it shows is the same, so an edit renders
only the edit's side.

| View | Opening it (both sides) | An edit beside Before | The edit's render |
| --- | --- | --- | --- |
| Before Only | 10 ms | 10 ms | 1283×1924 |
| Left / Right | 25 ms | 12 ms | 1028×1541 |
| Top / Bottom | 43 ms | 18 ms | 1283×1924 |
| Split (line at 30%) | 32 ms | 18 ms | 1283×1924 |

Copy After Settings to Before, an edit, then Swap: the edit takes Before's
Exposure back, as one step, "Swap Before and After Settings". The views
themselves were not looked at on this run; the layout's arithmetic is a
core test.

- A fitted side renders at its pane's long edge, so Top / Bottom renders a
  portrait photo larger than its half shows, as the single view does for a
  photo narrower than the screen.

## Import from the Photos library (2026-11-08)

Photos' own picker (no access to the library needed) hands over each
picked photo's RAW original as it is (`preferredAssetRepresentationMode =
.current`, `loadFileRepresentation` for `public.camera-raw-image`). Each is
copied into a folder of its own under the app's temporary folder, as two
originals can share a name, then the core's import moves them into
`Photos/<year>/<date>/`, leaving out what the catalog has by content; the
temporary folder goes afterwards. Photos without a RAW original are counted
and left out.

| Check | Where | Result |
| --- | --- | --- |
| Two originals both named `IMG_0001.DNG` and a third that is one of them again | Mac, core test | two imported, one left out, both moved |
| `-photos-import-test`: a copy of a catalog photo staged as the picker stages one | M1 iPad Pro | `0 imported · 1 already in the catalog; photos 54 -> 54; staging left false` |

Not checked yet: the picker itself, which takes a finger.

## The import screen (2026-11-08)

Import › From a Card or Folder lists the folder and everything in it
(`Library::scan_for_import`), grouped by the day each photo is filed under,
which the core now reports with each photo, so the screen's days are the
folders the import makes. Photos already in the catalog are dimmed and
can't be chosen; the new ones start chosen.

| Check | Where | Result |
| --- | --- | --- |
| Each listed photo's day is the folder it lands in | Mac, core test | passes |
| `-import-screen-test`: the catalog's own photo folder listed as a card | M1 iPad Pro | `54 photos in 5 days, 0 new, 0 chosen; listed in 114 ms; thumbnail 171×256` |

Not checked yet: the screen by hand, and a real card.

## Swiping between photos (2026-11-08)

In the editor, a swipe across the photo at Fit, or the ‹ › buttons, saves
the edit and opens the previous or next photo in the grid's order (its
filters and sort as they are). A failed save stays on the photo and says
so. The core's edit state now says which photo it belongs to, so the panels
show nothing of the last photo while the next one opens.

| Check | Where | Result |
| --- | --- | --- |
| The edit state names its photo | Mac, core test | passes |
| `-open 407A5994.CR3 -loupe-test` | M1 iPad Pro | `AITEST-407A5994.CR3 (edit of it, 966 ms) then 407A5994.CR3 (edit of it, 746 ms)`: half-size decode and edit loaded |

- **Found on the iPad:** `-develop-test` and `-mask-test` left their steps
  applied, and ending the app from the Mac saves what is shown, so the
  tested photo kept the test's edit. Both now go back to the edit as
  opened when they're done.

## Reset Settings and Clear History (2026-11-08)

Reset Settings (the toolbar's ↺) goes back to the photo's starting
settings as one step, as on the desktop. Clear History (History's menu)
keeps the edit and drops its steps; the save stores the History as
cleared.

| Check | Where | Result |
| --- | --- | --- |
| Reset, then Clear History, saved and reopened | Mac, core test | the edit kept, no steps |
| `-open 407A5994.CR3 -reset -clear-history -set-exposure 0.75 -save`, then relaunched | M1 iPad Pro | `EDIT exposure +0.75; steps ["Exposure +0.75"]; applied 1; unsaved false` both times: the photo the earlier test runs had edited, put back |

## Sort orders (2026-11-08)

The grid sorts by RAWmakase's own orders, moved into `rawmakase-catalog`
on the fork for it, either way.

| Check | Where | Result |
| --- | --- | --- |
| File Name by digits' value, Rating reversed | Mac, core test | `a`, `IMG_9`, `IMG_10`; the five-star photo first |
| `-sort-test` | M1 iPad Pro | each of the ten orders lists, Capture Time reversed starts with the newest |

## Metadata and keywords (2026-11-08)

The Info sheet (a photo's menu, the editor's ⓘ, or several photos chosen
in the grid) edits what the catalog keeps as the desktop's Metadata and
Keywording panels do: text overrides the file's own, an emptied field is
cleared, keywords are hierarchical ("Places > Berlin"). Camera info is read
from the file the first time, as the desktop reads it (a RAW through its
decoder, moved into `rawmakase-decode` on the fork), and kept.

| Check | Where | Result |
| --- | --- | --- |
| Per photo, survives reopening, a keyword taken off one photo stays on another, emptied is cleared; camera info read once | Mac, core tests | pass |
| `-metadata-test` on the test copy | M1 iPad Pro | title and `Safelight test > Info` read back; camera `EOS R5, 40mm F1.4 DG HSM \| Art 018, 1/60 sec at f/5.6, 40 mm, ISO 1600, 5464 × 8192`; both taken off again |

- **Found on the iPad:** reading a CR3's camera info from its EXIF alone
  found none (the desktop opens a RAW through its decoder), and the empty
  result was kept; an empty record is now read again.

## Virtual copies (2026-11-08)

A photo's menu, and the editor's ⧉, make a virtual copy as the desktop
does: the same file with its own edit, metadata and name, "Copy N". A
copy's menu renames it, makes it the master or removes it (after asking;
its file stays). The editor saves first and goes on with the copy.

| Check | Where | Result |
| --- | --- | --- |
| A copy starts with the master's edit, keeps its own, survives Set Copy as Master; a master isn't removed; the file stays | Mac, core test | passes |
| `-virtual-copy-test` on the test copy | M1 iPad Pro | `AITEST-407A5994.CR3 · Copy 1` of the same file, renamed `· Safelight test`; photos 54 → 55 → 54 |

## Deleting, renaming and moving (2026-11-08)

Through RAWmakase's file operations, which plan, journal every disk step
and let the catalog follow only files proven where they went. iPadOS gives
an app no system Trash, so deleted files go to Recently Deleted in the
app's storage until it is emptied.

The engine leaves RAWmakase's own data folder alone, and on the iPad the
photo folder was inside it (`Application Support/RAWmakase/Photos`). It now
lives beside it (`Application Support/Photos`): an existing library's
folder is moved there once, by one rename, and the catalog follows it.

| Check | Where | Result |
| --- | --- | --- |
| Delete takes a master's virtual copies and fills Recently Deleted until emptied; Remove leaves the file; rename, new folder, move and folder rename keep the photo and its rating; the old layout moves out with its photos | Mac, core tests | pass |
| First launch after the move | M1 iPad Pro | `LIBRARY 54 photos`; `407A5994.CR3` opens with `Exposure +0.75` |
| `-file-ops-test` | M1 iPad Pro | new folder 2, back 2, rename 2, rename back 2 (the test copy with its virtual copy), remove copy 1; 54 photos |

- **Found on the iPad:** every move and rename was refused as
  `Protected`, the photo folder being inside RAWmakase's data folder; the
  Mac's tests use a data folder elsewhere and didn't see it.
- The test leaves an empty "Safelight Test" folder: file operations never
  delete a folder.

## The Neural Engine (2026-11-09)

M1 iPad Pro, Release build, ONNX Runtime 1.23, a Canon CR3 (`-denoise-measure`,
`-selection-measure`), one run each. Differences are against the CPU's output.

| Model | CPU | CoreML plan | Time | Against the CPU |
|---|---|---|---|---|
| AI Denoise | 42.6 s | ML Program, GPU (in use) | 11.9 s | max 1.1e-6, 154 dB |
| | | ML Program, Neural Engine | 22.1 s | max 1.6e-6, 150 dB |
| | | ML Program, GPU and Neural Engine | 11.2 s | as the GPU's: it runs there |
| | | NeuralNetwork, any units | 61–68 s | identical: CoreML took none of it |
| Select Subject | 2.3–2.4 s | any | — | SAM 2's graphs do not load on CoreML |
| | | NeuralNetwork, DETR only | 2.5–2.6 s | IoU 1.0 |
| | | ML Program, DETR only | — | fails: a dimension fixed at 16 |
| Select Sky | 6.5–7.0 s | ML Program, Neural Engine | 10.1 s, then 37.6 s | both empty: the photo has no sky |
| | | ML Program, GPU or both | — | fails to build its plan |
| | | NeuralNetwork, any units | — | the app is killed at the memory limit |

The Neural Engine runs half precision only, so it takes nothing of a
single-precision ML Program; AI Denoise's activations overflow half precision
anyway. Nothing changes: AI Denoise stays on CoreML's GPU and the selections on
the CPU. The DETR-only rows were measured with a build that kept SAM 2 on the
CPU, which was not kept.
