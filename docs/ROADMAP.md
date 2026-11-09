# Roadmap

What the iPad app does, what the desktop RAWmakase does that it doesn't yet,
and the order to close the gaps. Checked off as it lands; "S", "M" and "L"
are rough sizes. "Wiring" means the model or engine has it already and only
the core's API and the iPad's screens are missing.

Last checked against the desktop: 2026-11-09 (fork `ios/build-crates`).

## Done

- [x] Catalog in the app's storage, the desktop's format; survives relaunch
      and the app's container moving
- [x] Live preview on Metal, Fit and 100%, pinch and pan
- [x] Every Develop slider: all of the desktop's sliders but the two
      incremental white balance ones, which Temp and Tint stand in for, and
      all 17 mask sliders
- [x] Panel switches, B&W, lens profile on/off, chromatic aberration
- [x] Crop, Straighten, Rotate, Flip, aspect ratios
- [x] Undo and Redo, History saved with the edit; Reset Settings, Clear
      History
- [x] Presets (built-in and imported Lightroom XMP), Copy/Paste Settings
      with groups, paste onto several photos
- [x] Export (JPEG, TIFF, PNG, size, quality) and the share sheet
- [x] Ratings, flags, colour labels, filters, collections, Quick Collection
- [x] Brush, linear and radial masks, with the Pencil's pressure, Density
      and Auto Mask; refining a Subject with taps; Heal and Clone spots
- [x] AI Denoise; Select Subject, Sky and Background (models downloaded on
      request, ONNX Runtime embedded)
- [x] Import core: list a card or folder, find duplicates by content, copy
      into year/date folders, progress and Cancel

## Now: the Develop gaps a Lightroom user misses most

- [x] History list, tap a step to go back to it
- [x] White balance presets and the eyedropper
- [x] Auto Settings and Auto white balance
- [x] Point curve, RGB and per channel, built-in curves
- [x] Profile browser and Profile Amount
- [x] Importing profiles (DCP, XMP looks, `.cube` LUTs as sRGB looks) from the Files
      app, files or folders, as the desktop's Import Profiles does
- [x] Histogram and clipping warnings
- [x] Mask math (add, subtract, intersect, invert), mask invert and Amount,
      Color and Luminance Range masks
- [x] Editing a placed gradient with handles; showing the mask over the
      photo (at Fit; not yet at 100%)
- [x] Saving presets, Update, Rename, Delete; Preset Amount; Snapshots
- [x] Before/After: Before only, side by side, top and bottom, split (a
      line to drag), Copy and Swap, a History step or snapshot to Before

## Next: import and library, for daily use

- [x] Import screen: a card's or folder's photos by the day they're filed
      under, those already in the catalog marked, all new chosen to start,
      a day at a time, progress and Cancel
- [x] Import from the Photos library, RAW originals
- [x] Swipe between photos in the editor, in the grid's order
- [x] Compare and Survey: a Select and a Candidate side by side, or the
      chosen photos at once, each with its flag and stars
- [x] Lightroom's sort orders, either way
- [x] Stacks: group, unstack, move to top, expand and collapse, Auto-Stack by
      capture time
- [x] Metadata (title, caption, creator, copyright), keywords and camera
      info, for one photo or several
- [x] Virtual copies: create, rename, set as master, remove
- [x] Delete photos (to Recently Deleted), remove from the library, rename
      and move photos, new and renamed folders

## Then: the rest of Develop

All of Develop's panels are on the iPad now, as the desktop has them:
the HDR switch (the photo shows its SDR rendition; HDR display is L),
Guided Upright, Lensfun's community profiles (downloaded on request),
Calibration's process version and camera exposure baseline, a Pet Eye's
catchlight, and the camera thumbnail. Per-slider Auto and resetting a panel
are Lightroom's, not RAWmakase's: not planned unless touch needs them.

- [ ] HDR display: the HDR master above SDR white on screens with headroom
      (L)

The desktop doesn't evaluate smart collections yet, and has no depth, object or people masks, no content-aware remove
and no soft proofing, so they aren't gaps.

## Export, fuller

- [x] Export presets (the desktop's export settings, by name), Lightroom's
      Rename To templates with an example, colour space, JPEG XL, Metadata
      Include and Remove Location, the Simple Copyright Watermark; through
      the desktop's export batch, with the last choices remembered
- [x] Filename templates (Lightroom's Filename Template Editor) and
      watermark presets, text or graphic, made on the iPad in RAWmakase's
      formats
- [ ] Output sharpening (the desktop has none yet)

## Sync

- [ ] Sync through S3-compatible storage, designed in [SYNC.md](SYNC.md):
      originals at home on a device (or in the bucket), per-album rules
      (Transit, Archive, Everywhere), XMP sidecars and a change log, in the
      background (L; phases 1–6 there; needs a bucket)
- [ ] Smart previews: edit without the original on the device (L)

## Platforms and polish

- [x] TestFlight: an App Store Connect record ("Safelight RAW"),
      Apple-managed signing, archives uploaded from the command line, an
      internal group
- [x] Huge files: linear DNGs (merged panoramas) open at half size in the
      reduced mode, a 271 MP panorama in 814 MB of pixels
- [x] Compatibility: `scripts/compat-check.sh` renders an edit made here in
      the desktop app, pixel for pixel the same
- [x] iPhone layout: the photo above the panel, the tools along the
      bottom, the Library on its grid (and a narrow iPad window the same)
- [ ] The interface modelled on Lightroom for iPad throughout (M–L): the
      Library and Develop's frame are (photo edge to edge, a panel per tool
      chosen on a rail); Edit's groups (Light, Color, Effects,
      Detail, Optics) and Crop are; Presets (with previews on
      the photo) and Heal are; the mask list floats over
      the photo; Cancel and Apply for tools; a double tap resets a slider; masks grouped as
      Lightroom's Masking panel, with a preview each and an overlay colour; Export as
      Lightroom's Save to Device, Share and Export As…
- [x] AI Denoise on CoreML: measured on an M1 iPad Pro, a 45 MP photo's
      output within 1.1e-6 of the CPU's, in 12.7 s against 39.9 s
- [x] AI Denoise on the Neural Engine: a half-precision Core ML conversion
      bundled with the app (app/Models), about 6× faster than CoreML's GPU
      (a 45 MP photo ready in about 5 s), 89.7 dB from the CPU's output; a
      progress bar while it runs
- [x] Select Sky on the Neural Engine: a half-precision Core ML conversion
      bundled with the app, made at build time (scripts/convert-sky-ane.py),
      about 18× faster than the CPU for 3:2, 4:3, 16:9 and square photos;
      other shapes stay on the CPU, and no download is needed for the ones it
      covers; it falls back to the GPU, then the CPU
- [ ] A better AI Denoise model: RawNIND falls short of Lightroom's Denoise
      on fine texture. Find a RAW-domain model with a licence we can ship
      that runs on the Neural Engine, and compare both on the same high-ISO
      photos against Lightroom (M–L; the model is RAWmakase's, so raise it
      upstream too)
- [ ] Select Subject and Background on the Neural Engine: SAM 2's graphs do
      not load on CoreML through ONNX Runtime; try them converted as Sky was
- [ ] Hand testing on the device: the Pencil, the Files and Photos pickers, the import screen,
      a real SD card, the Before/After views

## Desktop features not planned for the iPad

MIDI controllers, the MCP server and scripting, remote inference, the
command line and self-update are the desktop's.
