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

Each copy is RAWmakase's file with only paths and visibility changed. The comment at
its top names the upstream commit it was taken from (`ea3d0634` for all of them now).
The copies' own tests come with them.

| Safelight | RAWmakase |
| --- | --- |
| `core/src/edit_session/` | `src/edit_session/` |
| `core/src/edit_session/before_after.rs` | what `src/app/develop/before_after.rs` does to settings |
| `core/src/lensfun_install/` and `core/tests/lensfun/` | `src/lensfun_install.rs`, `src/lensfun_install/` and `tests/lensfun/` |
| `core/src/library_sort.rs` | `src/app/library/sort.rs` |

The copies don't follow upstream by themselves. At every update, list what upstream
changed in their originals since the recorded commit, and port it by hand:

```sh
cd vendor/rawmakase
git fetch https://github.com/pch/rawmakase.git main
git log --oneline ea3d0634..FETCH_HEAD -- src/edit_session \
    src/app/develop/before_after.rs src/lensfun_install.rs src/lensfun_install \
    tests/lensfun src/app/library/sort.rs
```

Then raise the commit named in each copy's first comment, and the one in this file.

## What the fork still changes

On top of upstream's `main`, as of 2026-11-10:

- **Crates upstream doesn't have.** `rawmakase-ai` holds AI Denoise's service, the
  model installer and what a selection records, all moved out of the desktop app
  (`e9c59a61`, `56bcb818`). This is the largest change left and the likeliest to
  conflict.
- **Code moved from the desktop app into existing crates.** Camera info into
  `rawmakase-decode`. Into `rawmakase-model`: Color Grading's wheel, Point Color's
  handles, duplicating a mask, crop guides, the Straighten angle, the curve's split
  limits, a mask raster's extent, spot spacing and Red Eye. Profile, lens profile and
  preset import into `rawmakase-interop`. These moves can conflict when upstream edits
  the same code; each should become a Safelight copy or go away.
- **Additions.** Preset folders (`7502a357`), export settings kept elsewhere
  (`a02ca0a9`), collections and sets written to the catalog (`aee18ea0`), and Core ML
  and Neural Engine loaders for AI Denoise, Sky, SAM 2 and DETR (`133ae07a`,
  `78689e14`, `c8b2be6f`, `545c736c`, `612d34bf`).
- **Changed behaviour.** Linear raw develops at half size in the reduced mode
  (`c5988a1f`), and AI Denoise falls back from the Neural Engine to the GPU before the
  CPU (`99a6d6e9`). The first changes what the desktop shows too.
- **iPad builds.** `3ea34bd3` and `08d82948`.

Edit History, Before/After, the Library's sort orders and the Lensfun installer used
to be moved into crates of their own on the fork. Since 2026-11-10 they are copies in
Safelight, and the fork has them as upstream does.
