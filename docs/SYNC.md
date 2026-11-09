# Sync: a self-hosted library across devices

Status: design, not built (2026-11-09). It replaces the two lines on sync in
[VISION.md](VISION.md) and the roadmap. Nothing here is implemented yet; the
[phases](#phases) say in which order it would be.

## What it is for

People leaving Lightroom's cloud want what it does for them (one library on
the iPad, the laptop and the desktop, culled and edited anywhere, with
nothing to press) without Adobe holding their photos. Safelight's sync does
that with storage the user owns: any S3-compatible bucket (AWS, Backblaze B2,
Cloudflare R2, Wasabi, a MinIO on a NAS). There is no Safelight server and no
account.

The requirements it is designed to:

1. **Tiered storage.** The bucket is, by default, a transfer layer, not the
   permanent home: originals live permanently on the user's own devices, on
   the desktop in the user's own folder structure. A library can instead make
   the bucket its home, as Lightroom's cloud is (the user's own setup is
   device-as-home; both are designed for).
2. **Rules per album** decide where originals stay: Transit, Archive or
   Everywhere ([Storage](#storage-home-and-rules)).
3. **Everything carries:** originals; per photo an XMP sidecar in the
   standard format (rating, pick/reject, label, keywords with their
   hierarchy, title, caption, location, and the edit as Camera Raw `crs:`
   settings); and a change log for what XMP can't carry (albums, stacks,
   virtual copies, which device changed what, when).
4. **Two ways:** culling, metadata and edits flow in every direction.
5. **No manual sync, ever:** no sync button; it runs in the background on
   every device.
6. **Conflicts resolve by fixed rules,** without asking, and nothing is lost.
7. **Works with RAWmakase on a computer** as it is today, through ordinary
   files ([Desktop](#the-desktop)).
8. Identifiers use the `com.leonjbergmann` prefix.

## Terms

- **Library:** one set of photos, albums and edits, synced through one
  bucket prefix. Each device has its own copy of the catalog (SQLite, never
  synced as a file).
- **Device:** an installation that syncs: Safelight on an iPad or iPhone, or
  the desktop sync service. Each has a random id (`device`), made once.
- **Home:** where a library's originals live permanently: one device (the
  default, the user's choice) or the bucket.
- **Bucket:** the S3-compatible storage, used through a prefix of the
  user's choice, so one bucket can hold several libraries.

## Three layers

| Layer | What | Read by |
| --- | --- | --- |
| 1. Files | Originals, and an XMP sidecar per photo | Any photo application: RAWmakase, Lightroom Classic, darktable, digiKam |
| 2. Change log | One small JSON record per change, per device | Safelight devices and the desktop service |
| 3. Caches | Snapshots of the merged log, previews and proxies | Safelight devices, to join and to show photos they don't hold |

The log is authoritative. Sidecars are written from it, so another
application sees the current state without knowing the log exists. Edits a
user makes to a sidecar in another application are read back by the desktop
service as changes ([Desktop](#the-desktop)).

## The bucket

```
<prefix>/
  library.json                   format version, home, default rule, minimum client
  originals/ab/abcdef….cr3       by SHA-256 of the file, extension kept
  sidecars/<photo>.xmp           the current sidecar of each photo
  log/<device>/<seq>.json        each device's changes, numbered from 1
  devices/<device>.json          name, kind, last seen, what it holds, its cursors
  blobs/<sha256>                 mask rasters and other binary parts of edits
  previews/<photo>-<rev>.jpg     2048 px previews, for devices without the original
  proxies/<photo>-<rev>.dng      smart previews, for editing without it (later)
  snapshots/<cursor>.json.zst    the merged state at a cursor, for joining
```

Every device writes only under its own `log/<device>/` and
`devices/<device>.json`, so no two devices ever write the same object. S3 has
no append and no atomic rename; one object per change, written once with
`If-None-Match: *`, needs neither. Originals and blobs are named by content,
so writing one twice is harmless. A device reads others' logs with
`ListObjectsV2` from its cursor (`start-after`), so a poll that finds nothing
new costs one request per device.

## Identity

- **Photo:** a random id (UUIDv7) made by the device that imports it, plus
  the SHA-256 of its original. A second device importing the same file
  finds the hash and adopts the id instead of making a second photo.
- **Virtual copy:** its own photo id, with the master's id.
- **Album and stack:** random ids.
- **Device:** a random id, kept in the device's own storage, never in the
  bucket's catalog.

Paths are never synced as absolute paths. The home device keeps each
photo's place in its own folders; others place originals where they like.

## The change log

One record per change. A gesture is one record (a slider drag is one edit
record, not hundreds); the app writes records when an edit is saved, as it
already saves edits.

```json
{
  "format": 1,
  "device": "6f1c…",
  "seq": 4182,
  "at": "2026-11-09T14:31:07.412Z/0003/6f1c…",
  "kind": "attrs",
  "photo": "01933a…",
  "set": { "rating": 4, "flag": "pick" }
}
```

`at` is a hybrid logical clock: the device's wall time, a counter for
changes within the same millisecond or behind a clock it has seen, and the
device id. Every device orders any two records the same way, whatever their
clocks say, and a device whose clock runs fast can't make its changes win
for longer than it takes the others to see them.

| Kind | Carries |
| --- | --- |
| `photo.added` | Hash, size, file name, capture time, the importing device; the folder it belongs in on the home |
| `photo.placed` | Moved or renamed on the home: the new folder and name |
| `photo.deleted` / `photo.restored` | Tombstone, and its undo |
| `attrs` | Any of rating, flag, label, title, caption, creator, copyright, location, capture time |
| `keywords` | Keyword paths added and removed |
| `edit` | The edit as `crs:` settings (and Safelight's own, in RAWmakase's namespace), the blobs it needs, the edit it was based on |
| `album.*` | Created, renamed, moved, deleted; its rule |
| `album.members` | Photos added and removed |
| `stack.*` | Grouped, ungrouped, top changed |
| `copy.*` | Virtual copy made, set as master |
| `original.*` | `confirmed` (a device holds the original, verified by its hash), `released` (it no longer does), `wanted` (a device asks the home for it) |

A device that reads a kind or a format it doesn't know holds that record and
everything after it from the same device until it is updated; it never skips
one. `library.json` names the oldest client format the library accepts.

### Merging

Each device applies others' records to its own catalog in `at` order, in one
transaction per batch with the cursors it advanced, so a crash replays a
batch and a replay changes nothing. A record whose blobs haven't arrived waits
for them.

### Conflicts

All by fixed rules, the same on every device; nobody is asked.

- **Attributes and culling:** each field on its own, the later change wins
  (so a rating set on the iPad and a label set on the desktop both stay).
- **Keywords and album membership:** add and remove per keyword or per
  photo, the later of the two wins.
- **Edits:** the later edit wins. The other is kept: in the photo's History
  as a step named for its device ("Edit from MacBook Pro"), and when both
  edits were based on the same earlier edit, also as a virtual copy, so both
  stay visible side by side.
- **Delete against change:** a delete wins only when it is later; a later
  change to a deleted photo restores it. Deleted photos go to Recently
  Deleted on every device for 30 days, as deletes do today.

Nothing a user did is lost: a lost edit is in History or a copy, a deleted
original is in Recently Deleted, and the log keeps every record until
[trimmed](#joining-and-trimming).

### Joining and trimming

A device joining a library reads the newest snapshot, then the log after it.
Any device that has merged everything may write a snapshot when the log has
grown (about 10,000 records since the last one, or a day of changes). Logs are
trimmed only behind the oldest cursor every known device has reached; a
device that hasn't been seen for 90 days is told to join again rather than
holding the log forever.

## Storage: home and rules

The library's **home** is a setting: one device (the default) or the bucket.

Each album has a **rule**; photos in no album have the library's default
rule (Transit unless changed). A photo in several albums gets the strongest
of their rules: Everywhere, then Archive, then Transit.

| Rule | In the bucket | On the home | On other devices |
| --- | --- | --- | --- |
| **Transit** (default) | Until the home has confirmed it, then deleted | Kept | Kept while it fits; previews otherwise |
| **Archive** | Kept | Kept | Fetched when opened, released when space is needed |
| **Everywhere** | Kept | Kept | Kept, iPad included |

With the **bucket as home**, Transit keeps the original in the bucket for
good (nothing is ever deleted from it), and devices hold originals only by
the rules, fetching the rest when opened, as Lightroom's cloud does.

The order of a Transit original's life, with a device as home:

1. The importing device uploads it (`originals/<hash>`), then writes
   `photo.added`.
2. The home downloads it, checks the hash, writes it into its folder,
   syncs it to disk, and writes `original.confirmed`.
3. Only then does any device delete it from the bucket. While the home is
   offline it stays there, however long that is.

A device that wants a Transit original the bucket no longer has writes
`original.wanted`; the home uploads it again when it next runs, and it is
deleted again once the asking device confirms. Until then the device shows
the photo's preview, and later edits it from its proxy.

**When a device fills up** it releases originals only where a rule allows:
never an Everywhere photo, never on the home, and only when another copy is
confirmed (on the home or in the bucket). Least recently opened goes first.
It says so in the Library's status line ("412 originals kept in the cloud to
save space"); there is no dialog.

## Transfers without a button

**iPad and iPhone.** Uploads and downloads of originals, previews and
proxies run as background `URLSession` transfers
(`com.leonjbergmann.safelight.transfer`), which the system carries on while
the app is suspended and prefers on Wi-Fi and power. Reading and writing the
log, merging, and scheduling transfers run when the app opens and in a
`BGProcessingTask` (`com.leonjbergmann.safelight.sync`) that asks for network
access. Nothing waits for the user; the Library shows what is still on its
way.

**Desktop.** A small service, `safelight-sync`, runs as a launch agent
(`com.leonjbergmann.safelight.sync`) whether or not any photo application
is open. It shares the iPad's sync code (Rust), watches the home folders
(FSEvents) for files and sidecars that change, and polls the bucket about
once a minute (more often while a device is active, as `devices/*.json`
says). The same program runs on Linux (systemd user service) and on a NAS.

**Credentials** (endpoint, region, bucket, prefix, key) stay in the
Keychain (access group `com.leonjbergmann.safelight`), never in the bucket.

## The desktop

The home device on a computer is the folder tree, as the user organises it:
the service files new photos into `<home>/<year>/<date>/` (the same layout
Safelight's import uses), unless the user moves them. Moving or renaming in
Finder is noticed and becomes `photo.placed`.

**With RAWmakase as it is today** (checked against its docs and code on
2026-11-09):

- Originals arrive as ordinary files with their sidecars. **Add Photo
  Folder** and **Synchronize Folder** add the new ones, with the sidecar's
  title, caption, copyright, creator, capture time, location, keywords,
  rating and colour label (`xmp:Label`). Lightroom keeps picks and rejects
  out of XMP, so Safelight's sidecars also write the flag as digiKam's
  `PickLabel`, which RAWmakase reads.
- **Not carried into RAWmakase:**
  - the edit: Add Photo Folder doesn't read a sidecar's `crs:` settings
    (RAWmakase reads `crs:` for presets and batch renders, and writes it on
    export, but not from a photo's sidecar into its catalog);
  - anything for photos already in the catalog: adding a folder again
    doesn't read their sidecars, and **Read Metadata from Files…** has to be
    chosen by hand.
- **Not carried back:** RAWmakase never writes sidecars, so culling and
  edits made in RAWmakase don't reach the other devices through files.

So layer 1 gives RAWmakase new photos with their descriptive metadata,
nothing more. Two-way work with a RAWmakase library needs a bridge to its
catalog. Two ways are possible, and the [phases](#phases) leave the choice
until it is needed:

- **The service reads and writes RAWmakase's catalog itself,** through the
  same `rawmakase-catalog` crate Safelight already uses, while RAWmakase
  isn't running (it holds a lock while it is), so RAWmakase sees the
  changes when it next opens. This needs nothing from RAWmakase.
- **An extension,** if RAWmakase's extension plan
  ([pch/rawmakase#967](https://github.com/pch/rawmakase/issues/967)) ever
  gains a contribution for it. Its `source` contribution reports arriving
  files only, so it covers the originals but not attributes or edits; a sync
  contribution isn't in that plan, and nothing here assumes one will be.

Either way the format stays independent of any one application and
documented here, so other tools can read it.

**Lightroom Classic** reads layer 1 as it is: with "Read metadata from
files", its catalog takes the sidecars' metadata and edits.

## What this borrows

The change log follows the design BlueBlock proposed for a multi-computer
RAWmakase catalog
([pch/rawmakase#187](https://github.com/pch/rawmakase/issues/187)): one file
per change rather than an appended file, a local SQLite catalog on each
machine, snapshots only as a join cache, paths kept per machine, unknown
changes held rather than skipped, and the losing edit kept. It differs where
a bucket differs from a share: each device writes under its own prefix
instead of taking a lease, and changes are ordered by a hybrid clock and
merged per field rather than refused by a base revision.

## Phases

1. **Format and bucket.** This document reviewed; `library.json`, the record
   schema and a conformance test suite in the repository; a bucket set up in
   the iPad's settings, with its credentials checked.
2. **iPad to the desktop, one way.** The iPad uploads originals and
   sidecars; the desktop service files them into the home folders and
   confirms; Transit deletes from the bucket. RAWmakase adds them with
   Synchronize Folder. Useful on its own: shoot, cull and edit on the iPad,
   find it all at home.
3. **Two ways between Safelight devices.** The full log, merging, conflicts,
   snapshots; iPad and iPhone in sync.
4. **The desktop both ways.** Sidecar changes made by other applications read
   back; the bridge to a RAWmakase catalog, decided then.
5. **Rules and space.** Archive and Everywhere, fetching on open, releasing
   when full, previews for what isn't on the device.
6. **The bucket as home,** and smart previews (proxies) for editing without
   the original.

## Open questions

- **Client-side encryption.** It would keep the bucket's provider from
  seeing photos, but sidecars and originals would then be unreadable to
  other tools while in the bucket. A per-library choice, later?
- **Several homes.** A NAS and a laptop both keeping everything: one home
  with Everywhere on the laptop covers it for now.
- **Bucket costs.** Polling and per-change objects are cheap on most
  providers; requests to measure on the first real library.
- **Sharing with another person.** Out of scope: one user, their devices.
