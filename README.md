# RAWmakase for iPad

*Lightroom's way of working on the iPad, without Lightroom.*

RAWmakase for iPad is a native iPad app for developing RAW photos, built on
[RAWmakase](https://github.com/pch/rawmakase)'s engine: the same RAW
decoders, colour pipeline, catalog and edits as the desktop app, under a
touch interface written in SwiftUI. A catalog you edit on the iPad opens in
RAWmakase on a Mac, Linux or Windows computer, and renders the same there.

It is an independent, personal project, MIT licensed like RAWmakase and not
affiliated with its maintainer.

> [!WARNING]
> **Early and in active development.** There is no App Store build: you
> build it yourself (below) and install it on your own iPad. Things move
> fast and some are rough; keep your originals elsewhere too until sync
> lands.

## What we want

Lightroom's cloud workflow is good: import on the iPad, edit anywhere, the
same photos on every device. It also means a subscription, Adobe's cloud as
the only copy of your library, and edits that only Adobe's apps read. The
goal is that workflow on your own terms:

- **Edit on the iPad as you would in Lightroom**: the same panels and
  sliders, masks with the Pencil, presets, AI Denoise and AI masks, export
  and share.
- **One library on iPad, iPhone and Mac**, synced through storage you
  choose (any S3-compatible bucket), with the originals and the edits yours.
- **Open formats**: the catalog is RAWmakase's SQLite catalog, Lightroom
  XMP presets and settings read as Lightroom means them, and originals are
  never changed.
- **Everything on the device**: rendering, AI Denoise and the AI masks run
  on the iPad; no photo leaves it unless you sync or share it.

## How

- **RAWmakase's crates, not a rewrite.** The app uses the desktop's own
  Rust crates (decoding, the renderer, the catalog, edit resolution, export,
  AI) from a fork, kept to the few changes iPadOS needs so RAWmakase's
  updates still merge: a submodule at `vendor/rawmakase` on the fork's
  [`ios/build-crates`](https://github.com/ljbergmann/rawmakase/tree/ios/build-crates)
  branch.
- **A small Rust core with a Swift API.** `core/` composes those crates
  into what the app calls, through [UniFFI](https://mozilla.github.io/uniffi-rs/):
  the library in the app's storage, the open edit with its History, the live
  preview, import and export. The rules of an edit stay in Rust, in one place.
- **A live preview on the GPU.** The engine renders straight into the
  view's Metal layer, at 60 fps while a slider moves on an M1 iPad Pro.
- **A native interface.** SwiftUI, modelled on Lightroom for iPad, with the
  Pencil's pressure for brushes.
- **Sync by changes, not by files.** Each device will write its own change
  records and upload originals by content hash; the SQLite file itself is
  never synced. Then smart previews, to edit without the original on the
  device.

The step-by-step plan is [docs/PLAN.md](docs/PLAN.md); what's left, in
order, is the [roadmap](docs/ROADMAP.md).

## What works today

- **Library**: a catalog in the app's storage; import from the Files app (a
  card, a drive, iCloud Drive); thumbnails; ratings, flags, colour labels,
  filters, collections and the Quick Collection.
- **Develop**: every slider of RAWmakase's Develop; profiles and Profile
  Amount; white balance presets and eyedropper; Auto; the point curve; Crop,
  Straighten, Rotate and Flip; Undo, Redo and a History you can step back
  through.
- **Masks and retouching**: brush (with the Pencil's pressure), linear and
  radial masks with their own sliders; Select Subject, Sky and Background;
  Heal and Clone.
- **AI Denoise**, on the device (about 38 s for a 45 MP photo on an M1 iPad
  Pro). The models are downloaded once, on request, and checked.
- **Presets** (RAWmakase's and your Lightroom XMP presets) and Copy/Paste
  Settings, onto one photo or many.
- **Export** to JPEG, TIFF or PNG, and the share sheet.

What's missing compared with the desktop, and what comes next (an import
screen, the Photos library, Before/After, mask math, sync…), is in the
[roadmap](docs/ROADMAP.md). Measurements are in
[MEASUREMENTS.md](MEASUREMENTS.md).

## Build

You need a Mac with Xcode, [XcodeGen](https://github.com/yonaskolb/XcodeGen)
and rustup's stable toolchain with the iPad targets:

```sh
git clone --recurse-submodules https://github.com/ljbergmann/rawmakase-ios
cd rawmakase-ios
rustup target add --toolchain stable aarch64-apple-ios aarch64-apple-ios-sim
scripts/build-core.sh    # the Rust core and its Swift bindings
scripts/build-ort.sh     # Microsoft's ONNX Runtime for iOS, for the AI features
xcodegen generate
open Rawmakase.xcodeproj
```

Then run it on your iPad from Xcode, with your own team for signing. The
scripts use rustup's toolchain explicitly: a Homebrew `rustc` on `PATH` has
no iPad targets. Run `scripts/build-core.sh` again after changing `core/` or
the crates.

From the command line (the iPad unlocked, Developer Mode on):

```sh
xcodebuild -project Rawmakase.xcodeproj -scheme Rawmakase -configuration Release \
  -destination 'id=<the iPad's UDID>' -derivedDataPath build/DerivedData \
  -allowProvisioningUpdates DEVELOPMENT_TEAM=<your team ID> build
```

## Layout

| Path | What |
| --- | --- |
| `app/Sources/` | The SwiftUI app; `app/Generated/` holds the Swift bindings the build writes |
| `core/` | The Rust core the app calls (`rawmakase-core`) |
| `vendor/rawmakase/` | RAWmakase, as a submodule on the fork's `ios/build-crates` branch |
| `scripts/` | Building the core and ONNX Runtime |
| `project.yml` | The Xcode project, for XcodeGen |
| `docs/` | The plan and the roadmap |

## Development

```sh
cd core && cargo test                 # the core's tests, on the Mac
cd vendor/rawmakase && make check     # after changing RAWmakase's crates
```

The app has launch options that do something and print what happened, for
checking it on a device without touching the screen (with `devicectl`, put
them after `--`):

| Option | What it does |
| --- | --- |
| `-import-documents` | Moves the photos in the app's Documents folder into the catalog |
| `-open <file>` | Opens a photo of the catalog in the editor |
| `-sweep`, `-pan` (with `-open`, optionally `-full`) | Measures live rendering while a slider moves, or at 100% while panning |
| `-set-exposure <stops>`, `-save` | Edits and saves, to check edits survive a relaunch |
| `-develop-test`, `-mask-test`, `-ai-test` | Exercise white balance, Auto, the curve and History; masks and Heal; the AI models |
| `-export <file>` | Exports a photo at full size, timed |
| `-benchmark`, `-allocprobe`, `-thumbnails`, `-rate <file> <stars>` | Rendering and memory measurements, thumbnails, ratings |

`-benchmark` renders each RAW in Documents on the GPU and the CPU and writes
`<name>.ipad.ppm` for comparing with the Mac's render (`cargo run --release
--example render` in `core/`, with `HOME` pointing at an empty folder so
neither side reads installed profiles).

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) says how to
build, where things live and what a change must keep; the
[roadmap](docs/ROADMAP.md) is a good place to pick something.

## License

MIT, as RAWmakase ([LICENSE](LICENSE)). RAWmakase is by its
[contributors](https://github.com/pch/rawmakase/graphs/contributors).
ONNX Runtime is Microsoft's (MIT); the AI models keep their own licences,
shown before each download.
