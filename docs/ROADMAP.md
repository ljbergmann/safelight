# Roadmap

What the iPad app does, what the desktop RAWmakase does that it doesn't yet,
and the order to close the gaps. Checked off as it lands; "S", "M" and "L"
are rough sizes. "Wiring" means the model or engine has it already and only
the core's API and the iPad's screens are missing.

Last checked against the desktop: 2026-11-08 (fork `ios/build-crates`).

## Done

- [x] Catalog in the app's storage, the desktop's format; survives relaunch
      and the app's container moving
- [x] Live preview on Metal, Fit and 100%, pinch and pan
- [x] Every Develop slider: all of the desktop's sliders but the two
      incremental white balance ones, which Temp and Tint stand in for, and
      all 17 mask sliders
- [x] Panel switches, B&W, lens profile on/off, chromatic aberration
- [x] Crop, Straighten, Rotate, Flip, aspect ratios
- [x] Undo and Redo, History saved with the edit
- [x] Presets (built-in and imported Lightroom XMP), Copy/Paste Settings
      with groups, paste onto several photos
- [x] Export (JPEG, TIFF, PNG, size, quality) and the share sheet
- [x] Ratings, flags, colour labels, filters, collections, Quick Collection
- [x] Brush, linear and radial masks, with the Pencil's pressure; Heal and
      Clone spots
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
- [ ] Importing profiles (DCP, XMP looks, `.cube`) from the Files app (M)
- [x] Histogram and clipping warnings
- [x] Mask math (add, subtract, intersect, invert), mask invert and Amount,
      Color and Luminance Range masks
- [x] Editing a placed gradient with handles; showing the mask over the
      photo (at Fit; not yet at 100%)
- [x] Saving presets, Update, Rename, Delete; Preset Amount; Snapshots
- [x] Before/After: Before only, side by side, top and bottom, split (a
      line to drag), Copy and Swap, a History step or snapshot to Before

## Next: import and library, for daily use

- [ ] Import screen modelled on Lightroom for iPad: a card's or folder's
      grid, already-imported photos marked, select all new, progress (M)
- [x] Import from the Photos library, RAW originals
- [ ] Import a Lightroom Classic catalog (`.lrcat`) or a Lightroom cloud
      library, with edits (the desktop has both; M–L)
- [ ] Loupe with swipe between photos, compare and survey views (M)
- [ ] Sort orders, stacks, smart collections (M)
- [ ] Metadata (title, caption, creator, copyright) and keywords (M)
- [ ] Virtual copies (M)
- [ ] Delete photos, move and rename files and folders (M)

## Then: the rest of Develop

- **Basic:** Treatment Auto, the HDR switch (HDR display is L), Fit Camera
  Look, per-slider Auto, panel and slider reset by double tap
- **Tone Curve:** saved curves, the parametric graph with split points,
  Levels Black and White point, the targeted adjustment tool
- **Color:** Point Color, targeted HSL and B&W, the mixer's All view,
  grading wheels
- **Lens and Transform:** choosing a lens profile and Lensfun, Defringe,
  Upright and Guided Upright, Constrain Crop, Calibration's process version
- **Crop:** Auto Straighten and the ruler, overlays (thirds, golden ratio…),
  a custom ratio
- **Masks:** brush Density and Auto Mask, duplicate, reset sliders, refining
  a Subject with taps
- **Retouch:** brushed spots, feather and opacity, moving or deleting a spot,
  Visualize Spots, Red Eye and Pet Eye
- **Presets:** favorites and search, Paste from Previous
- **View:** zoom levels between Fit and 100%, photo info overlay, Reference
  View, camera thumbnail

The desktop has no depth, object or people masks, no content-aware remove
and no soft proofing, so they aren't gaps.

## Export, fuller

- [ ] Export presets, file naming templates, metadata options, output
      sharpening, colour space, watermarks (the desktop has all; M)

## Sync

- [ ] Sync through S3-compatible storage: per-device change records,
      originals by content hash, never the SQLite file (L; needs a bucket)
- [ ] Smart previews: edit without the original on the device (L)

## Platforms and polish

- [ ] iPhone layout (M–L)
- [ ] The interface modelled on Lightroom for iPad throughout (M–L)
- [ ] AI Denoise on CoreML (GPU / Neural Engine); measure against the CPU (S–M)
- [ ] Hand testing on the device: the Pencil, the Files and Photos pickers,
      a real SD card, the Before/After views

## Desktop features not planned for the iPad

MIDI controllers, the MCP server and scripting, remote inference, the
command line and self-update are the desktop's.
