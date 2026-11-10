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

## Copies of RAWmakase code in Safelight

The first four are RAWmakase's files with only paths and visibility changed. Those in
`core/src/from_desktop/` are upstream's logic taken out of its window code, with
methods on RAWmakase's types written as functions. `ai_denoise` is the one copy Safelight
changed on purpose: it picks the Neural Engine, loads the model Safelight's way, and
falls back to the GPU before the CPU. AI Denoise's maths (the model, its tiles, the
gain fit and the Amount blend) stays in `rawmakase-inference` and `rawmakase-engine`,
so it follows upstream by itself. Each names the upstream commit it
was taken from (`ea3d0634` for all of them now), and their tests come with them.

| Safelight | RAWmakase |
| --- | --- |
| `core/src/edit_session/` | `src/edit_session/` |
| `core/src/edit_session/before_after.rs` | what `src/app/develop/before_after.rs` does to settings |
| `core/src/lensfun_install/` and `core/tests/lensfun/` | `src/lensfun_install.rs`, `src/lensfun_install/` and `tests/lensfun/` |
| `core/src/library_sort.rs` | `src/app/library/sort.rs` |
| `core/src/from_desktop/photo_info.rs` | reading a photo's info in `src/catalog_session/backfill.rs` |
| `core/src/from_desktop/grading.rs` | Color Grading's wheel in `src/app/develop/panels/color_grading.rs` |
| `core/src/from_desktop/bulk_import.rs` | `src/app/presets/bulk_import.rs`, with presets imported into a chosen folder |
| `core/src/from_desktop/point_color.rs` | Point Color's range handles in `src/app/develop/panels/point_color_panel.rs` |
| `core/src/from_desktop/masks.rs` | Duplicate in `src/app/develop/tools/mask_tool.rs`, a raster's extent in `src/app/develop/tools/subject_mask/mod.rs` |
| `core/src/from_desktop/crop_guides.rs` | the crop guides in `src/app/develop/tools/crop_tool.rs` |
| `core/src/from_desktop/geometry.rs` | the Straighten ruler in `src/app/develop/tools/crop_tool.rs`, the curve's splits in `src/app/ui/widgets.rs` |
| `core/src/from_desktop/retouch.rs` | spot spacing in `src/app/develop/tools/retouch_tool.rs`, Red Eye in `src/app/develop/tools/red_eye_tool.rs` |
| `core/src/from_desktop/ai_denoise/` | `src/ai_denoise/`, with Safelight's Neural Engine changes |
| `core/src/from_desktop/model_install/` | `src/model_install/` |
| `core/src/from_desktop/selection.rs` | what a selection records, in `src/app/develop/tools/subject_mask/worker.rs` |

The copies don't follow upstream by themselves. At every update, list what upstream
changed in their originals since the recorded commit, and port it by hand:

```sh
cd vendor/rawmakase
git fetch https://github.com/pch/rawmakase.git main
git log --oneline ea3d0634..FETCH_HEAD -- src/edit_session \
    src/app/develop/before_after.rs src/lensfun_install.rs src/lensfun_install \
    tests/lensfun src/app/library/sort.rs src/catalog_session/backfill.rs \
    src/app/develop/panels/color_grading.rs src/app/presets/bulk_import.rs \
    src/app/develop/panels/point_color_panel.rs src/app/develop/tools/mask_tool.rs \
    src/app/develop/tools/subject_mask/mod.rs src/app/develop/tools/crop_tool.rs \
    src/app/ui/widgets.rs src/app/develop/tools/retouch_tool.rs \
    src/app/develop/tools/red_eye_tool.rs src/ai_denoise src/model_install \
    src/app/develop/tools/subject_mask/worker.rs
```

Most of those files hold window code too, so many of the commits listed won't touch
what was copied: read each one's diff for the copied parts.

Then raise the commit named in each copy's first comment, and the one in this file.

## What the fork still changes

On top of upstream's `main`, as of 2026-11-10 (18 files):

- **Additions to `rawmakase-inference`.** Finding ONNX Runtime in an iOS app
  (`08d82948`), a Neural Engine provider, CoreML plans and `measure_with_coreml` for
  measuring them, and what an app needs to run AI Denoise, Sky, SAM 2 and DETR its own
  way (`78689e14`, `c8b2be6f`, `545c736c`, `612d34bf`). Safelight's Neural Engine code
  can't reach those internals any other way.
- **Other additions.** Export settings kept elsewhere and a watermark found by name
  (`a02ca0a9`), and collections and sets written to the catalog (`aee18ea0`).
- **Changed behaviour.** Linear raw develops at half size in the reduced mode
  (`c5988a1f`). This changes what the desktop shows too.
- **iPad builds.** `3ea34bd3`: Apple platforms, not only macOS, where macOS's paths
  apply.

The fork used to move more out of the desktop app. Edit History, Before/After, the
sort orders and the Lensfun installer were crates of their own, and so was
`rawmakase-ai`, with AI Denoise's service, the model installer and the selection
records. The helpers above were in `rawmakase-model`, `rawmakase-decode`,
`rawmakase-interop` and `rawmakase-engine`. Since 2026-11-10 those are copies in
Safelight, and the fork has the code as upstream does.
