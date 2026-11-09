# RAW+JPEG: the camera's JPEG as the reference look

Status: design, not built (2026-11-09). Waiting for approval before any code.

## What it is for

A photographer shooting RAW+JPEG gets two files per shot: the RAW, and the
JPEG the camera made from it with its own picture style (Fujifilm's film
simulations, Canon's Picture Styles, Nikon's Picture Controls). That JPEG is
the look they chose in the field. They want to keep it with the RAW and use
it, not edit it:

1. **Import the pair as one photo.** The JPEG comes in with its RAW as the
   RAW's camera reference, not as a second photo in the grid.
2. **See it.** The grid thumbnail and the editor's first preview show the
   camera's JPEG, as the camera rendered it.
3. **Compare.** Put the camera JPEG beside the edit, or over it, while
   editing.
4. **Start from it.** Fit Camera Look fits a profile to the paired JPEG, so
   the edit starts at the straight-out-of-camera (SOOC) look and goes on from
   there.

Today, a card or folder import brings in both files as two unrelated photos;
an import from Photos brings in the RAW only. A RAW's embedded preview stands
in for the camera's JPEG: it drives the thumbnail, the Camera Thumbnail view
and Fit Camera Look. The embedded preview is the same rendering, but often
smaller (1616 pixels in many Sony ARWs), sometimes missing or JPEG XL, and
for Adobe DNGs Adobe's rather than the camera's, so Fit Camera Look refuses
those.

## How RAWmakase treats a pair, and why the JPEG stays out of the catalog

RAWmakase on a computer has no RAW+JPEG pairs. **Add photo folder** catalogs
both files as photos of their own, each with its own XMP (`B.xmp` is the
RAW's; the JPEG reads its embedded XMP and `B.JPG.xmp`;
`sidecar_tests.rs`). The Lightroom importer brings no JPEG sidecars, since
Lightroom keeps them out of its catalog. RAWmakase's file operations move,
rename and trash only a photo's XMP sidecars and old edit files with it
(`file_ops/companions.rs`), never a JPEG.

So the catalog has no place for a JPEG that belongs to a RAW without being a
photo, and Safelight must not invent one: the [contribution
guide](../CONTRIBUTING.md#what-a-change-must-keep) keeps the catalog the
desktop's, with nothing only Safelight understands in it. That rules out:

- **The JPEG beside the RAW in `Photos/<day>/`.** `Photos` is a root of the
  catalog: the next `add_new_photos` (and the desktop's own rescan) would
  catalog the JPEG as a second photo. Keeping it out would mean changing what
  the desktop's folder import does, for everyone.
- **A new catalog table or field** for the pairing. The desktop would ignore
  it and drop it on its next migration or copy.
- **Two photos in a stack** (the RAW on top, collapsed). This is the one
  option the desktop understands as it is, but the JPEG would still be a
  photo: it counts in the library, filters, Select All, export and delete;
  its rating and flag drift from the RAW's; and nothing marks which stacked
  JPEG is "the camera's". The tester asked for exactly the opposite.

### The choice: a reference file beside the catalog, named by the RAW's content

The camera's JPEG is kept in `References/` beside the catalog and the
`Photos` folder, in Safelight's own storage, as the guide asks of
Safelight-only data:

```
Application Support/Safelight/
  Photos.rawmakase
  Photos/2026/2026-11-09/DSCF1234.RAF
  References/<sha256 of DSCF1234.RAF>.jpg
```

It is named by the RAW's full checksum, which the import already reads for
every photo it brings in and saves in the catalog's checksum table (the
desktop's own `FileChecksum`, kept for finding moved files). Naming it by
content rather than by path means:

- **Renaming and moving a photo need nothing.** The RAW's content doesn't
  change, so its reference is found again; Safelight's file operations don't
  have to follow along, and a crash between two moves can't separate them.
- **Virtual copies** share their master's file, and so its reference.
- **Deleting** moves the RAW into Recently Deleted and leaves its reference.
  **Empty Recently Deleted** then removes every reference whose checksum no
  photo in the catalog has. Nothing else deletes a reference.
- **Importing the same RAW again** (a duplicate) finds its reference there,
  or adds the JPEG to it when the RAW has none yet.
- **The desktop** opening the catalog sees the RAW alone, as it would have
  without Safelight, and edits render the same (`scripts/compat-check.sh`
  is unaffected: nothing in the catalog changes). Sync carries `References/`
  as Safelight's own files; [SYNC.md](SYNC.md) gets a line saying so.

A catalog made on the desktop from a RAW+JPEG folder has the JPEG as a photo
of its own. For such a photo, the RAW's camera JPEG is that catalogued JPEG
(same folder, same name, same capture time), used in place, not copied. One
function answers "the camera JPEG of photo N" for both cases, in the core.

## The pairing rule

A RAW and a JPEG are a pair when they are in the same folder of the source,
their names match but for the extension (case ignored, as cameras write
`DSCF1234.RAF` and `DSCF1234.JPG`), the JPEG's extension is `.jpg` or
`.jpeg`, and their capture times are the same to the second, or one of them
has none. Both cameras' files of one shot are written with one EXIF time;
the time guards against reused numbers (a counter that rolled over, two
cameras with the same prefix in one folder). HEIF from cameras that write it
beside RAW (Canon's and Sony's HEIF mode) pairs by the same rule.

From Photos, a RAW+JPEG pair is one asset with two resources (the JPEG as
`.photo`, the RAW as `.alternatePhoto`), so there is nothing to match: the
asset is the pair.

The rule lives in the core (`core/src/pairs.rs`), not in the fork: the
desktop has no pairs, so it has nothing to share.

## Import

The import screen, for a card or folder, lists a pair as one tile with a
"RAW+JPEG" badge, and offers what to do with the JPEGs of pairs, remembered
between imports in Safelight's own settings:

- **Keep as camera reference** (the default): the RAW is imported and its
  JPEG copied to `References/`.
- **Ignore**: the RAW only.
- **Import as photos of their own**: both, as today and as the desktop does.
  The JPEG is then still found as the RAW's camera JPEG, by the desktop rule
  above.

A JPEG with no RAW is a photo of its own as before. The counts and the
progress bar include the reference copies, so a cancel between a RAW and its
JPEG keeps the RAW and drops the JPEG (the next import of the card adds it to
the RAW, as for a duplicate). A reference is written through a temporary name
and renamed into place, as originals are, so a cut-off copy never looks like
a reference.

From Photos, the picker offers the asset's JPEG. The same choice is offered
in the import sheet: Keep as reference stages the RAW and the JPEG resource
(both read through `PHAssetResourceManager`, which needs the library read
access the RAW of a pair needs already); Import as photos of their own makes
the JPEG a photo too (new for Photos: today a JPEG from Photos is reported as
"not RAW").

The import job takes each RAW with its optional reference as one item
(`ImportItem { path, reference: Option<path> }`), so the reference is
captured with its RAW when the import starts, not looked up again afterwards.

## Thumbnail and first preview

`Library::thumbnail` makes a photo's grid thumbnail from its camera JPEG
when it has one, else from the RAW's embedded preview, as today. It takes
the photo's id rather than a path, since the camera JPEG is the photo's, and
the cache key names the source file (the reference's or the RAW's) with its
size and modification time, so adding a reference later makes a new
thumbnail. The editor's first preview, shown until its first render, is that
thumbnail, so it follows.

A camera JPEG is up to 60 MP, about 180 MB decoded, and the core's JPEG
decoder (zune-jpeg) can't decode at a reduced size. So the full JPEG is
decoded once per reference, one at a time (the grid's decode gate), into a
4096-pixel copy in Caches that the thumbnails, the Camera JPEG view and
Before/After then read; iPadOS emptying Caches only means it is made again.
The footprint is measured with `Memory.footprint()` and recorded in
MEASUREMENTS.md.

The import screen's tiles stay the RAW's embedded preview: they are read
before anything is imported, and the difference doesn't matter for choosing
what to import.

## Compare

**Camera JPEG view.** The Camera Thumbnail view (`CameraThumbnailView`,
`core/src/camera_thumbnail.rs`) shows the camera JPEG when the photo has
one, labelled "Camera JPEG: Velvia, DSCF1234.JPG, no edits"; without one it
is the embedded preview, labelled as today. Its menu item becomes "Camera
JPEG" when there is one.

**Before/After against the camera.** Before/After (`core/src/compare.rs`)
gets a second Before: **Camera JPEG**, beside **Before settings** (today's).
Every layout works with it: Before alone, side by side, and split. A fourth
layout, **Overlay**, puts the camera JPEG over the edit at an opacity the
user drags, for judging tone and colour on the same pixels.

The camera JPEG is framed like the edit's crop: its crop rectangle, angle
and orientation, applied to the JPEG, which matches the RAW's default render
(the camera's own lens correction, uncropped). Upright, Transform and a lens
profile are not applied to it, so in a split or overlay of an edit using
them the two halves don't line up; the label says so. The live view draws
the JPEG as a texture with the edit, as it draws Before now, at screen size
(never the full JPEG on the GPU).

The Copy Before/After commands do nothing with Camera JPEG as Before: the
JPEG has no settings to copy. Fit Camera Look is the way to bring its look
over.

## Fit Camera Look from the camera JPEG

Fit Camera Look (`LiveView::fit_camera_look`, which runs RAWmakase's
`camera_look::fit`) fits a profile to the RAW's embedded preview. With a
camera JPEG it fits to that instead: larger, and available where the
embedded preview isn't usable (missing, too small, JPEG XL). The fitted
profile is saved whole in the recipe (`rawmakase:FittedProfile` in XMP), so
the result renders the same on the desktop, which never needs the JPEG.

This needs the fork: `camera_look::fit` reads the preview from the RAW
itself (`pairs::preview`). The change, one commit on `ios/build-crates`:

- `FitInputs` gets `reference: Option<&Path>`, a JPEG to fit to in place of
  the embedded one. `pairs::preview` reads it with the same colour-space rule
  (EXIF and ICC, sRGB when it states none) and refuses it with the same
  reasons, plus "the JPEG doesn't show this RAW" when it fails to align
  (the alignment and quality gates are unchanged: a JPEG cropped to another
  aspect in the camera is cut to the RAW's, as embedded previews are).
- `preview_srgb` gets the same `reference`, for the Camera JPEG view.
- The fit cache key (`FitKey`) includes the reference's identity when there
  is one. The cache serves only fitted defaults, which keep using the
  embedded preview; a reference changes nothing for them.

Kept small, as the guide asks: no new decoder, no new solver, a parameter
threaded to where the preview is read. Upstream RAWmakase could use it too
(the desktop's folder import does see pairs), but that's for RAWmakase to
want; nothing here depends on it.

The core captures the RAW path, the reference path and their identities when
the fit starts, and refuses the result if either changed meanwhile, as it
does for the RAW today.

## Not in this

- Rating, flag or label from the JPEG: the RAW's XMP and embedded rating are
  the photo's, as they are now.
- Showing a camera JPEG for a JPEG photo, or pairing a RAW with an edited
  JPEG export: a JPEG whose EXIF `Software` names an editor is still paired
  (the rule can't tell an edited copy reliably), but Fit Camera Look's
  alignment gate refuses one that was cropped or retouched.
- Exporting the camera JPEG: it's the camera's file, in `References/`; the
  Files app doesn't see it.

## Phases

Each lands with its core tests, and the app built and checked on the
simulator (a pair is made there from a folder of files; a RAW+JPEG asset in
Photos can only be checked on the iPad).

1. **Pairs and references in the core.** `pairs.rs` (the rule), the
   `References/` store, `camera_jpeg(photo)` with the desktop-pair fallback,
   the sweep on Empty Recently Deleted. Card and folder import with the
   three choices. Tests: pairing by name and time, a cancel between a RAW
   and its JPEG, a duplicate RAW gaining a reference, rename and move
   keeping it, delete then empty removing it, a desktop pair found in place.
2. **Thumbnail and first preview** from the camera JPEG, with its footprint
   measured.
3. **Camera JPEG view** and **Before/After** with Camera JPEG and Overlay.
4. **Fit Camera Look** from the camera JPEG: the fork commit, then the core.
5. **Photos**: the pair's JPEG resource, and the choice in the import sheet.
6. Roadmap, README launch options, SYNC.md's line on `References/`.
