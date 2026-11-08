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

## As close to RAWmakase as it can be

Safelight is RAWmakase on Apple's phones and tablets, not a separate photo
editor. It is its own project with its own name, but it is coupled to
RAWmakase on purpose:

- **One engine.** Decoding, rendering, the catalog, edit resolution, presets,
  export and AI are RAWmakase's crates, used as they are. A photo renders the
  same in Safelight and in RAWmakase, and an improvement to RAWmakase's
  engine is an improvement to Safelight.
- **One catalog.** Safelight keeps RAWmakase's catalog format. A library
  edited on the iPad opens in RAWmakase on a computer, and the other way
  round.
- **Shared code goes to RAWmakase.** Anything that isn't about the iPad's
  screens (an edit's rules, AI Denoise's service, import, and in time sync)
  belongs in RAWmakase's crates, where the desktop can use it too. The fork
  that Safelight builds from stays a small set of such changes, kept so
  RAWmakase's updates merge, and offered back to RAWmakase where they fit.
- **Follow RAWmakase.** When RAWmakase releases, Safelight moves to it.
  Where the two would differ, RAWmakase's behaviour wins unless a touch
  screen needs something else.

The Mac is where this pays off: the aim is for RAWmakase on the Mac to join
the same synced library, rather than a separate Mac app.

## How it gets there

1. **An iPad app for daily use** (now): every Develop tool a Lightroom user
   reaches for, a proper import from cards, folders and the Photos library,
   and a library you can live in.
2. **Sync.** Each device writes its own change records to the bucket and
   uploads originals by content hash, never the SQLite file; each device
   merges what the others wrote into its own catalog. Designed so
   RAWmakase's desktop can take part.
3. **Smart previews.** Edit a photo whose original isn't on this device,
   from a small proxy, as Lightroom does; download the original only to
   export.
4. **iPhone.** The same app, laid out for a phone: cull, rate, quick edits,
   share.
5. **Mac.** RAWmakase on the Mac in the same synced library.
6. **Moving in from Lightroom.** Bring a Lightroom library across, with its
   edits.

## What Safelight isn't

- **Not a fork of RAWmakase's engine.** It doesn't change how photos render;
  that happens in RAWmakase.
- **Not a cloud service.** There is no Safelight server or account; sync
  is between your devices and your storage.
- **Not a Lightroom clone.** It follows Lightroom's way of working and reads
  Lightroom's settings, but it is its own app, with its own name and look,
  and no Adobe code, profiles or assets.
