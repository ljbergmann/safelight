# Safelight's RAWmakase fork

Safelight builds on RAWmakase's crates from a fork, the `vendor/rawmakase`
submodule on [`ljbergmann/rawmakase`'s `ios/build-crates`](https://github.com/ljbergmann/rawmakase/tree/ios/build-crates).
The aim is that Safelight can always take RAWmakase's newest version: merge it, build,
test. That works only while the fork stays close to upstream, so:

- **Safelight's own needs go in Safelight's core**, never into RAWmakase's code.
- **Where Safelight needs code from RAWmakase's desktop app**, it keeps a copy. The
  desktop app's crate links its window libraries and doesn't build for iOS. The copies
  are listed below.
- **The fork only adds things.** Any change to RAWmakase's code, even a small one, is
  agreed with the maintainer of Safelight first. Nothing from the fork goes upstream
  without them either.

## What Safelight takes from RAWmakase's crates

Most of what Safelight needs is in RAWmakase's own crates, used as they are:
`rawmakase-model`, `-interop`, `-catalog`, `-decode`, `-engine`, `-export` and
`-inference`, and since 0.8 `rawmakase-denoise` (AI Denoise's service, run with
Safelight's Neural Engine loader), `rawmakase-model-install` (Safelight's model
packs, `core/src/models.rs`, in the folders RAWmakase uses) and
`rawmakase-selection` (Select Subject, Sky and Background).

## Copies of RAWmakase code in Safelight

The first four are RAWmakase's files with only paths and visibility changed. Those in
`core/src/from_desktop/` are upstream's logic taken out of its window code, with
methods on RAWmakase's types written as functions. Each names the upstream commit it
was taken from, and their tests come with them.

| Safelight | RAWmakase | Taken at |
| --- | --- | --- |
| `core/src/edit_session/` | `src/edit_session/` | `9d17dc99` (0.8.0) |
| `core/src/edit_session/before_after.rs` | what `src/app/develop/before_after.rs` does to settings | `9d17dc99`, unchanged since `ea3d0634` |
| `core/src/lensfun_install/` and `core/tests/lensfun/` | `src/lensfun_install.rs`, `src/lensfun_install/` and `tests/lensfun/` | `9d17dc99` |
| `core/src/library_sort.rs` | `src/app/library/sort.rs` (Custom Order is there, but the grid shows no album's order yet) | `9d17dc99` |
| `core/src/from_desktop/photo_info.rs` | reading a photo's info in `src/catalog_session/backfill.rs` | `9d17dc99` |
| `core/src/from_desktop/grading.rs` | Color Grading's wheel in `src/app/develop/panels/color_grading.rs` | `9d17dc99` |
| `core/src/from_desktop/bulk_import.rs` | `src/app/presets/bulk_import.rs`, with presets imported into a chosen folder | `9d17dc99` |
| `core/src/from_desktop/point_color.rs` | Point Color's range handles in `src/app/develop/panels/point_color_panel.rs` | `9d17dc99` |
| `core/src/from_desktop/masks.rs` | Duplicate in `src/app/develop/tools/mask_tool.rs`, a raster's extent in `src/app/develop/tools/subject_mask/mod.rs` | `9d17dc99` |
| `core/src/from_desktop/crop_guides.rs` | the crop guides in `src/app/develop/tools/crop_tool.rs` | `9d17dc99` |
| `core/src/from_desktop/geometry.rs` | the Straighten ruler in `src/app/develop/tools/crop_tool.rs`, the curve's splits in `src/app/ui/widgets.rs` | `9d17dc99` |
| `core/src/from_desktop/retouch.rs` | spot spacing in `src/app/develop/tools/retouch_tool.rs`, Red Eye in `src/app/develop/tools/red_eye_tool.rs` | `9d17dc99` |
| `core/src/from_desktop/selection.rs` | what a selection records: `src/app/extensions/mask_generator/mod.rs`'s `image_key` and `input_identity`, the AI Masks extension's `ModelId` (`extensions/ai-masks/src/inference.rs`) | `9d17dc99` |

The copies don't follow upstream by themselves. At every update, list what upstream
changed in their originals since the recorded commit, and port it by hand:

```sh
cd vendor/rawmakase
git fetch https://github.com/pch/rawmakase.git main
git log --oneline 9d17dc99..FETCH_HEAD -- src/edit_session \
    src/app/develop/before_after.rs src/lensfun_install.rs src/lensfun_install \
    tests/lensfun src/app/library/sort.rs src/catalog_session/backfill.rs \
    src/app/develop/panels/color_grading.rs src/app/presets/bulk_import.rs \
    src/app/develop/panels/point_color_panel.rs src/app/develop/tools/mask_tool.rs \
    src/app/develop/tools/subject_mask/mod.rs src/app/develop/tools/crop_tool.rs \
    src/app/ui/widgets.rs src/app/develop/tools/retouch_tool.rs \
    src/app/develop/tools/red_eye_tool.rs src/app/extensions/mask_generator \
    extensions/ai-masks/src/inference.rs
```

Most of those files hold window code too, so many of the commits listed won't touch
what was copied: read each one's diff for the copied parts. Then raise the commit
named in each copy's first comment, and the ones in this file.

## What the fork still changes

On top of RAWmakase 0.8.0 (`9d17dc99`), as of 2026-11-25, 8 commits:

- **iPad builds.** Apple platforms, not only macOS, take macOS's folders and HEIC
  through ImageIO; ONNX Runtime is found in an iOS app's `Frameworks` folder.
- **Additions for an app's own models.** A `NeuralEngine` provider in
  `rawmakase-inference`, so Neural Engine results have their own cache key. In
  `rawmakase-selection`: `panoptic_input_size`, `panoptic_labels`,
  `Embedding::from_features` and `Session::analyze_from` for Select Subject's networks
  run on Core ML, `sky_input_size` made public and `sky_from_answers` for Select Sky's.
  `Session::analyze` is split in two for `analyze_from`, its behaviour unchanged.
- **Other additions.** Export settings kept elsewhere and a watermark found by name.
- **Changed behaviour.** Linear raw beyond 100 megapixels (merged panoramas) develops
  at half size in the reduced mode, so the iPad never holds a panorama's full size in
  floats. This applies on the desktop too; smaller linear raw is as upstream has it.

The old branch is tagged `ios/2026-11-25`. Since 0.8 the fork no longer writes
collections (the catalog has its own), and the CoreML plans and `measure_with_coreml`
are gone with the measurements that used them (MEASUREMENTS.md keeps their results).
