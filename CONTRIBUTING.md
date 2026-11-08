# Contributing to Safelight

Thanks for helping. This app is a SwiftUI front end over RAWmakase's own Rust
crates, so most of what makes a good change here is what makes one in
RAWmakase: read its [contribution guide](vendor/rawmakase/CONTRIBUTING.md)
too. This page says what's different on the iPad. What's left to build, and
in which order, is in [docs/ROADMAP.md](docs/ROADMAP.md): a good place to
pick something. Why it's built this way is in [docs/VISION.md](docs/VISION.md).

## Build and run

You need Xcode, XcodeGen and rustup's stable toolchain with the iPad targets
(the README's [Build](README.md) section has the commands):

```sh
git clone --recurse-submodules https://github.com/ljbergmann/safelight
rustup target add --toolchain stable aarch64-apple-ios aarch64-apple-ios-sim
scripts/build-core.sh    # the Rust core and its Swift bindings
scripts/build-ort.sh     # ONNX Runtime for iOS, for the AI features
xcodegen generate
open Safelight.xcodeproj
```

Run `scripts/build-core.sh` again after changing `core/` or the crates.

## Find your way

- `app/Sources`: the SwiftUI app. It draws screens and passes taps on; it
  decides nothing about an edit.
- `core/`: the Swift-facing API (UniFFI). It composes RAWmakase's crates and
  owns what the iPad needs that the desktop's app crate keeps to itself: the
  live view on Metal, the open edit, the library in the app's storage, import.
- `vendor/rawmakase`: RAWmakase, as a submodule on the fork's
  `ios/build-crates` branch. Its [code map](vendor/rawmakase/docs/code-map.md)
  and [architecture](vendor/rawmakase/docs/architecture.md) say where things
  live there.
- [docs/PLAN.md](docs/PLAN.md) explains how the port was laid out, and
  [MEASUREMENTS.md](MEASUREMENTS.md) records what was measured on the iPad.

## What a change must keep

RAWmakase's rules apply here as they are (saved edits keep working, domain
code stays free of the UI, background work always finishes, refactors apart
from behaviour changes, rendering references are evidence). On top of them:

- **One owner for each rule.** When the iPad needs something the desktop's
  app crate has (a step name, what Auto takes from its estimate), move it
  into a crate both use rather than copying it, as `rawmakase-edit-session`
  and `rawmakase-ai` were. The few places that still mirror the desktop say
  so in a comment.
- **Keep the fork small.** Changes under `vendor/rawmakase` go to the fork's
  `ios/build-crates` branch, one concern per commit, each passing that
  repository's `make check`, so RAWmakase's own updates still merge. Prefer
  moving code over changing it.
- **The catalog stays the desktop's.** A catalog made on the iPad must open
  in RAWmakase on a computer, and its edits render the same there.
- **Memory is tight.** An iPad may have 8 GB for everything. Measure the
  footprint of anything that decodes or renders at full size
  (`Memory.footprint()`), and record what you measured in MEASUREMENTS.md.
- **Swift threads are small.** Rust work called from Swift runs on the
  core's own 8 MB thread (`on_core_thread`), or Swift's 512 KB stacks
  overflow.

## Tests and checks

```sh
cd core && cargo test                     # the core
cd vendor/rawmakase && make check         # after changing the crates
```

A change to an edit starts with a core test of it (`core/src/edit.rs` has
the pattern: a catalog in a temporary folder with one of RAWmakase's chart
DNGs). Behaviour on the device is checked with the app's launch options,
listed in the README, which print what they did; screens are checked by
hand. Say which you ran, and what you couldn't (no iPad, no Pencil).

## Sending a change

- Open an issue first for anything large, so the approach can be agreed.
- One concern per pull request; say what changed, why, and how you checked
  it. A change that needs the fork gets a pull request there too.
- RAWmakase is someone else's project. Problems with Safelight, including
  ones in the fork, belong in Safelight's issues, not RAWmakase's. A fix
  that would help RAWmakase on its own merits can go to RAWmakase through
  its own guide, as any contribution would; don't ask its maintainer to
  take on Safelight's needs.
- Commit only the files you changed. Never commit RAW files, photos, Adobe
  profiles, Lightroom catalogs or paths from your own machine.
- Update the docs a change makes wrong: the roadmap when something lands,
  the README's launch options when you add one.

## Licence

MIT, as RAWmakase ([LICENSE](LICENSE)); contributions are accepted under the
same licence. Third-party code and models keep their own licences: ONNX
Runtime's comes with `build/onnxruntime.xcframework`, and the models' with
each download.
