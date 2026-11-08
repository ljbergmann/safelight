# RAWmakase on iPad: proof of concept plan

Goal: prove that a native SwiftUI iPad app can use RAWmakase's Rust crates to
open a catalog in the app's own storage, show its photos, and edit one photo
with a few Basic sliders, live, with Undo. End state of the project is a native
shell (Option B); this proof of concept builds nothing that would be thrown
away for it.

Out of scope: masks, AI (ONNX Runtime), presets, export, sync, Pencil,
external drives and SMB, iPhone, App Store.

Written on 2026-11-08 before Stage 0; what happened since is in the README's
status and in [MEASUREMENTS.md](../MEASUREMENTS.md).

## Where the code lives

- **Your fork of `pch/rawmakase`**, for changes to its crates. Keep them few
  and mechanical, so pch's updates still merge, and so some can be offered
  upstream.
- **A new repository, `rawmakase-ios`** (this one), for everything iPad-only: the Rust
  bridge crate and the Xcode project. It depends on the fork's crates by path
  or Git revision.

```
rawmakase-ios/
  core/          Rust: the Swift-facing API (staticlib + generated Swift)
  app/           Xcode project: SwiftUI app
  scripts/       build the xcframework for device + simulator
```

## Stage 0: tools (half a day)

- `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`
  (neither is installed on this Mac yet; Xcode 27.2 is).
- Pick the bridge. Recommendation: **UniFFI** (Mozilla): generates the Swift
  side from Rust, handles strings, records, errors and callbacks, and is used
  in production iOS apps. Alternatives: `swift-bridge`, or a hand-written C
  API with `cbindgen`. Decide after Stage 2's first call works.
- A device to test on. Note its chip and RAM; the memory numbers depend on it.

**Done when:** an empty Rust function returns a string to a SwiftUI label, on
the simulator and on the iPad.

## Stage 1: the crates build for iPad (1–3 days)

Build each crate below the app for `aarch64-apple-ios`, bottom up:

1. `rawmakase-cancel`, `-tiff`, `-protocol`, `-model`, `-raw`
   (pure Rust; expect few problems).
2. `rawmakase-interop`: `directories` and the data-folder rules in
   `rawmakase-protocol/src/paths.rs` assume desktop folders. On iPadOS the data
   folder is the app's Application Support; pass it in rather than detecting it.
3. `rawmakase-engine`: wgpu with Metal.
4. `rawmakase-decode`: its HEIC path and parts of `raster.rs` and
   `decode_cache.rs` are `cfg(target_os = "macos")`. ImageIO exists on iPadOS,
   so most changes are widening that to `any(target_os = "macos", target_os = "ios")`.
5. `rawmakase-fs` and `rawmakase-catalog`: `rawmakase-fs` is `cfg(unix)`,
   which includes iPadOS; check each call (`statfs`, exclusive renames, the
   SQLite lock check) at run time later, in Stage 4.

Leave out `rawmakase-inference`, `-remote`, `-export` and the app crate.

Expect a sweep for code that reads "Unix but not macOS" as Linux.

**Done when:** `cargo build --target aarch64-apple-ios` passes for those
crates, and the changes are a short, reviewable list in the fork.

## Stage 2: decode and render on the iPad, headless (2–4 days)

The command line's `rawmakase render` (in `src/main.rs`) is the path to copy:
open the photo (`photo::open`), take a recipe, develop it, get pixels.

- In `core/`, one function: `render(path, recipe_json, max_edge) -> RGBA8 + size`,
  on the CPU pipeline first, then the GPU one (`develop::PreviewRenderer`).
- Bundle three test RAWs of your own (one from a high-resolution camera, one
  Fuji X-Trans if you have one, one DNG). Never commit them.
- Show the result in a SwiftUI `Image`.

**Measure and write down:** time to open, decode and render each one, CPU vs
GPU; peak memory (Xcode's memory gauge / Instruments); whether the GPU path
starts at all, or falls back to the CPU.

**Done when:** all three render on the iPad with colours matching the Mac's
`rawmakase render` output of the same file and recipe.

**Stop and rethink if:** a high-resolution RAW is killed for memory, or a
render takes many seconds on the GPU.

## Stage 3: live preview in SwiftUI (3–5 days)

The hardest of the four questions.

- A `UIViewRepresentable` hosting a view whose layer is a `CAMetalLayer`;
  hand that layer to Rust, which makes the wgpu surface from it and draws the
  preview into it. SwiftUI never copies pixels.
- A slider in SwiftUI changes one recipe value; Rust re-renders at the view's
  size. Use the desktop's rule: only the latest request is kept, and a result
  for an older recipe is dropped.
- Pinch to zoom and pan in Swift, passed to Rust as the view's transform.

**Measure:** frames per second while dragging Exposure; latency from finger
to frame; memory with the photo open.

**Done when:** dragging Exposure feels live on a high-resolution RAW.

## Stage 4: catalog in the app's storage (3–5 days)

- Mechanical move in the fork: `src/edit_session` and `src/catalog_session`
  into crates of their own (they don't use egui; they sit in the app crate,
  which pulls in eframe). Keep their tests passing; offer the move to pch.
- Create or open a catalog in Application Support with `rawmakase-catalog`.
- Import through the Files picker (`UIDocumentPickerViewController`, copying
  files into the app's storage), then add them as a folder.
- A SwiftUI grid of thumbnails (the embedded JPEG, as `photo::open(..).thumbnail()`
  gives it) with ratings.
- Exercise `rawmakase-fs` here: renames, file ids and the SQLite lock check
  inside the sandbox.

**Done when:** you import 50 RAWs, quit the app, reopen it, and the grid and
ratings are still there.

## Stage 5: editing with Undo, through the model's rules (3–5 days)

- Six Basic sliders: Exposure, Contrast, Highlights, Shadows, Temp, Tint.
  Their ranges, defaults and side effects come from `rawmakase-model`'s
  parameters, not from Swift.
- Each slider gesture is one History step through `EditSession`; Undo and Redo
  buttons.
- Save to the catalog when the user leaves the photo, and when the app goes to
  the background (`scenePhase` → background). A save that fails keeps the
  photo open and says so.

**Done when:** edit a photo, kill the app from the app switcher, reopen: the
edit and its History are back. Open the same catalog file on the Mac's
RAWmakase (copied over): the edit renders the same.

## Total

Roughly 3–5 weeks of evenings-and-weekends pace is optimistic; plan for more.
Stages 2 and 3 carry most of the risk, so do them before polishing anything.

## What to decide after the proof of concept

- UniFFI or another bridge, now that it has been used for real.
- Whether the live preview needs a different split between Swift and Rust.
- Which of the fork's changes to offer pch (the `ios` cfgs, the session crates).
- Then step 2 of the larger plan: sync through S3 (per-device change records,
  originals by content hash; never the SQLite file itself).

## Known risks

| Risk | Where it shows | Fallback |
| --- | --- | --- |
| Memory on high-resolution RAWs | Stage 2 | Decode at reduced size for the preview; full size only at 100% |
| wgpu surface from a SwiftUI-hosted layer | Stage 3 | Render to a texture and present with Metal in Swift |
| Upstream moving fast | Every rebase | Keep fork changes small; offer them upstream |
| `rawmakase-fs` assumptions in the sandbox | Stage 4 | Disable file operations in the proof of concept |
