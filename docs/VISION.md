# Vision

Where Safelight is going, and the rules that keep it on course. The
[roadmap](ROADMAP.md) says what comes next; this page says why.

## The end goal

Leave Lightroom without giving up the way Lightroom works.

Lightroom's cloud workflow is the model: shoot, import the card on the iPad,
cull and edit there with the Pencil, carry on at home on the Mac, share a
favourite from the phone, and find the same photos with the same edits on
every device. What it costs is a subscription, Adobe's cloud as the only home
of the library, and edits that only Adobe's apps can read.

The end goal is that same workflow on your own terms:

- **Every device, one library.** iPad, iPhone and Mac show the same photos,
  ratings, collections and edits, kept in step without anyone thinking about
  it.
- **Your storage, your files.** Originals and edits live in storage you
  choose and pay for directly, any S3-compatible bucket, in formats you can
  read without Safelight. No account, no subscription, no server to run.
- **Lightroom's muscle memory.** The panels, sliders, masks, presets and
  shortcuts a Lightroom user already knows, and their Lightroom settings and
  presets meaning what they meant in Lightroom.
- **Coming from Lightroom is one step.** A Lightroom library, Classic or
  cloud, comes across with its photos, ratings, collections and edits.
- **Nothing leaves the device unasked.** Rendering, AI Denoise and the AI
  masks run where the photo is; photos travel only to your own storage or
  where you share them.

## Built on RAWmakase

Safelight exists because [RAWmakase](https://github.com/pch/rawmakase) does:
its engine is the hard part, and it is RAWmakase's work. Safelight is a
separate, independent project that builds on it, and tries to be a good
neighbour:

- **RAWmakase's engine, as it is.** Decoding, rendering, the catalog, edit
  resolution, presets, export and AI come from RAWmakase's crates. A photo
  renders the same in Safelight and in RAWmakase, and Safelight doesn't try
  to change how RAWmakase renders.
- **Compatible with RAWmakase.** Safelight keeps RAWmakase's catalog
  format, so a library edited on the iPad also opens in RAWmakase on a
  computer.
- **A small fork.** Safelight builds from a fork of RAWmakase with as few
  changes as possible, mostly moving code out of the desktop app into
  crates the iPad can use too, so that RAWmakase's own updates keep
  merging cleanly. If any of it is ever useful to RAWmakase, it's there to
  take; nothing is expected.
- **Following RAWmakase.** Safelight moves to RAWmakase's new releases, and
  where the two would behave differently, it does what RAWmakase does
  unless a touch screen needs something else.

RAWmakase's direction is its maintainer's to decide; Safelight's plans don't
depend on it. For the Mac, the simplest way is that the same library opens
in RAWmakase there, as it already can; how a Mac joins sync is still open.

## How it gets there

1. **An iPad app for daily use** (now): every Develop tool a Lightroom user
   reaches for, a proper import from cards, folders and the Photos library,
   and a library you can live in.
2. **Sync.** Each device writes its own change records to the bucket and
   uploads originals by content hash, never the SQLite file; each device
   merges what the others wrote into its own catalog.
3. **Smart previews.** Edit a photo whose original isn't on this device,
   from a small proxy, as Lightroom does; download the original only to
   export.
4. **iPhone.** The same app, laid out for a phone: cull, rate, quick edits,
   share.
5. **Mac.** The same library on the Mac; how, is still open.
6. **Moving in from Lightroom.** Bring a Lightroom library across, with its
   edits.

## What Safelight isn't

- **Not a fork of RAWmakase's engine, and not part of RAWmakase.** It
  doesn't change how photos render, and it isn't affiliated with RAWmakase
  or its maintainer.
- **Not a cloud service.** There is no Safelight server or account; sync
  is between your devices and your storage.
- **Not a Lightroom clone.** It follows Lightroom's way of working and reads
  Lightroom's settings, but it is its own app, with its own name and look,
  and no Adobe code, profiles or assets.
