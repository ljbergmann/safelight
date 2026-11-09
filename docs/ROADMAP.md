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

- **Basic:** Treatment Auto, the HDR switch (HDR display is L), Fit Camera
  Look, per-slider Auto, panel reset
- **Lens and Transform:** choosing a lens profile and Lensfun, Defringe,
  Upright and Guided Upright, Constrain Crop, Calibration's process version
- **Retouch:** moving a Pet Eye's catchlight (brushed spots, Feather and
  Opacity, moving, re-sourcing and deleting a spot, Visualize Spots, Red Eye
  and Pet Eye are done)
- **View:** camera thumbnail (zooming to any scale between Fit and 100%,
  the Info Overlay and Reference View are done)

The desktop doesn't evaluate smart collections yet, and has no depth, object or people masks, no content-aware remove
and no soft proofing, so they aren't gaps.

## Export, fuller

- [x] Export presets (the desktop's export settings, by name), Lightroom's
      Rename To templates with an example, colour space, JPEG XL, Metadata
      Include and Remove Location, the Simple Copyright Watermark; through
      the desktop's export batch, with the last choices remembered
- [ ] Custom filename templates and watermark presets made on the desktop
      (its data folder isn't on the iPad); output sharpening (the desktop
      has none yet)

## Sync

- [ ] Sync through S3-compatible storage: per-device change records,
      originals by content hash, never the SQLite file (L; needs a bucket)
- [ ] Smart previews: edit without the original on the device (L)

## Platforms and polish

- [ ] iPhone layout (M–L)
- [ ] The interface modelled on Lightroom for iPad throughout (M–L): the
      Library and Develop's frame are (photo edge to edge, a panel per tool
      chosen on a rail); Edit's groups (Light, Color, Effects,
      Detail, Optics) and Crop are; Presets (with previews on
      the photo) and Heal are; the mask list floats over
      the photo; Cancel and Apply for tools; a double tap resets a slider; masks grouped as
      Lightroom's Masking panel, with a preview each and an overlay colour
- [ ] AI Denoise on CoreML (GPU / Neural Engine); measure against the CPU (S–M)
- [ ] Hand testing on the device: the Pencil, the Files and Photos pickers, the import screen,
      a real SD card, the Before/After views

## Desktop features not planned for the iPad

MIDI controllers, the MCP server and scripting, remote inference, the
command line and self-update are the desktop's.
