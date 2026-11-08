import Foundation
import Observation
import UIKit

/// The catalog in the app's storage, and what the grid shows of it.
@MainActor @Observable
final class LibraryModel {
    private(set) var library: Library?
    private(set) var photos: [LibraryPhoto] = []
    private(set) var collections: [CollectionInfo] = []

    // The grid's filters, as Lightroom's Library Filter bar has them.
    var collection: Int64?
    var minRating: Int32 = 0
    /// 1 picked, -1 rejected, 0 unflagged; nil for any.
    var flagFilter: Int32?
    var labelFilter: String?
    var byCaptureTime = true

    /// The photos the filters let through, in the chosen order.
    var shown: [LibraryPhoto] {
        let members = collection.flatMap { id in collections.first { $0.id == id }.map { Set($0.photos) } }
        let kept = photos.filter { p in
            (members?.contains(p.id) ?? true)
                && p.rating >= minRating
                && (flagFilter.map { p.flag == $0 } ?? true)
                && (labelFilter.map { p.label == $0 } ?? true)
        }
        return byCaptureTime
            ? kept.sorted { ($0.captured, $0.filename) < ($1.captured, $1.filename) }
            : kept.sorted { $0.filename < $1.filename }
    }
    /// What an import in progress has done, for the toolbar.
    private(set) var importing: String?
    private(set) var error: String?
    /// The last import's timings, for measuring.
    private(set) var lastImport: String?
    /// What the last import brought in and left out, for the toolbar.
    var notice: String?
    /// An import that can be cancelled, while it runs.
    private(set) var importJob: ImportJob?

    /// Opens (or creates) the catalog in Application Support.
    func open() async {
        guard library == nil else { return }
        let fm = FileManager.default
        let data = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appending(path: "RAWmakase")
        let caches = fm.urls(for: .cachesDirectory, in: .userDomainMask)[0].appending(path: "RAWmakase")
        do {
            let opened = try await Task.detached { try Library.open(data: data.path, caches: caches.path) }.value
            library = opened
            await reload()
            // Dates for photos imported before they were read.
            Task {
                let found = (try? await Task.detached { try opened.fillCaptureTimes() }.value) ?? 0
                if found > 0 { await reload() }
            }
            let rated = photos.filter { $0.rating > 0 }.map { "\($0.filename)=\($0.rating)" }
            print("LIBRARY \(photos.count) photos; rated: \(rated)")
        } catch {
            self.error = "\(error)"
        }
    }

    func reload() async {
        guard let library else { return }
        do {
            photos = try await Task.detached { try library.photos() }.value
            collections = try await Task.detached { try library.collections() }.value
        } catch {
            self.error = "\(error)"
        }
    }

    /// Copies `files` (from the Files picker) into today's folder of the
    /// catalog's photo folder, then adds what is new to the catalog.
    func importFiles(_ files: [URL]) async {
        await importCopying(files, move: false)
    }

    /// Moves the RAWs in the app's Documents folder into the catalog: for
    /// filling an iPad from a Mac (Finder, or devicectl) without the picker.
    func importDocuments() async {
        await importCopying(Documents.photos(), move: true)
    }

    private func importCopying(_ files: [URL], move: Bool) async {
        guard let library, !files.isEmpty else { return }
        importing = "Copying \(files.count) photos…"
        let folder = URL(fileURLWithPath: library.photosFolder()).appending(path: Self.today())
        let started = ContinuousClock.now
        let outcome: Result<(Int, Added), Error> = await Task.detached {
            Result {
                let fm = FileManager.default
                try fm.createDirectory(at: folder, withIntermediateDirectories: true)
                var copied = 0
                for file in files {
                    let scoped = file.startAccessingSecurityScopedResource()
                    defer { if scoped { file.stopAccessingSecurityScopedResource() } }
                    let target = Self.unused(folder.appending(path: file.lastPathComponent))
                    if move { try fm.moveItem(at: file, to: target) } else { try fm.copyItem(at: file, to: target) }
                    copied += 1
                }
                return (copied, try library.addNewPhotos())
            }
        }.value
        importing = nil
        switch outcome {
        case .success(let (copied, added)):
            let seconds = Double((ContinuousClock.now - started).components.attoseconds) / 1e18
                + Double((ContinuousClock.now - started).components.seconds)
            lastImport = "\(copied) files copied, \(added.added) added to the catalog in \(Int(added.ms)) ms, \(String(format: "%.1f", seconds)) s in all"
            print("IMPORT \(lastImport!)")
            await reload()
        case .failure(let error):
            self.error = "\(error)"
        }
    }

    func setImporting(_ text: String?) {
        importing = text
        if text != nil { notice = nil }
    }

    /// Runs the core's import of `paths` (dated folders, duplicates by
    /// content left out) with progress in the toolbar and Cancel; returns
    /// what it did, in words.
    func runImport(_ paths: [String], mode: ImportMode) async -> [String] {
        guard let library else { return ["No catalog"] }
        let job = ImportJob.start(library: library, paths: paths, mode: mode, skipDuplicates: true)
        importJob = job
        while !job.finished() {
            importing = "Importing \(job.done()) of \(job.total())…"
            try? await Task.sleep(for: .milliseconds(200))
        }
        importJob = nil
        importing = nil
        await reload()
        var notes = ["\(job.imported()) imported"]
        if job.skipped() > 0 { notes.append("\(job.skipped()) already in the catalog") }
        if job.done() < job.total() && job.error() == nil { notes.append("cancelled") }
        if let error = job.error() { notes.append("stopped: \(error)") }
        return notes
    }

    func finishImport(_ note: String) {
        importing = nil
        lastImport = note
        notice = note
        print("IMPORT \(note)")
    }

    /// `-rate <file name> <stars>`: rates a photo without touching the
    /// screen, for checking that ratings survive a relaunch.
    func rateFromArguments() {
        let args = ProcessInfo.processInfo.arguments
        guard let i = args.firstIndex(of: "-rate"), i + 2 < args.count,
              let stars = Int32(args[i + 2]),
              let photo = photos.first(where: { $0.filename == args[i + 1] }) else { return }
        setRating(photo, to: stars)
        print("RATED \(photo.filename) \(stars)")
    }

    func setFlag(_ photo: LibraryPhoto, to flag: Int32) {
        guard let library else { return }
        do {
            try library.setFlag(id: photo.id, flag: flag)
            if let i = photos.firstIndex(where: { $0.id == photo.id }) { photos[i].flag = flag }
        } catch { self.error = "\(error)" }
    }

    func setLabel(_ photo: LibraryPhoto, to label: String) {
        guard let library else { return }
        do {
            try library.setLabel(id: photo.id, label: label)
            if let i = photos.firstIndex(where: { $0.id == photo.id }) { photos[i].label = label }
        } catch { self.error = "\(error)" }
    }

    func inQuickCollection(_ photo: LibraryPhoto) -> Bool {
        collections.first { $0.quick }?.photos.contains(photo.id) ?? false
    }

    func setQuickCollection(_ ids: [Int64], _ on: Bool) {
        guard let library else { return }
        do {
            try library.setQuickCollection(ids: ids, on: on)
            Task { await reload() }
        } catch { self.error = "\(error)" }
    }

    func setRating(_ photo: LibraryPhoto, to rating: Int32) {
        guard let library else { return }
        do {
            try library.setRating(id: photo.id, rating: rating)
            if let i = photos.firstIndex(where: { $0.id == photo.id }) { photos[i].rating = rating }
        } catch {
            self.error = "\(error)"
        }
    }

    // MARK: Thumbnails

    private let thumbnails = NSCache<NSString, UIImage>()
    private let decoding = ThumbnailGate(limit: 4)

    /// The photo's thumbnail, `edge` pixels on its long side.
    func thumbnail(_ photo: LibraryPhoto, edge: UInt32) async -> UIImage? {
        await thumbnail(path: photo.path, edge: edge)
    }

    /// The thumbnail of any photo file, in the catalog or not (a card's).
    func thumbnail(path: String, edge: UInt32) async -> UIImage? {
        let key = "\(path)-\(edge)" as NSString
        if let image = thumbnails.object(forKey: key) { return image }
        guard let library else { return nil }
        let gate = decoding
        let image: UIImage? = await Task.detached(priority: .utility) {
            await gate.wait()
            defer { Task { await gate.signal() } }
            return (try? library.thumbnail(path: path, edge: edge)).flatMap { UIImage(data: $0) }
        }.value
        if let image { thumbnails.setObject(image, forKey: key) }
        return image
    }

    nonisolated private static func today() -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: Date())
    }

    /// `url`, or `name-2.ext`, `name-3.ext`… when it is taken.
    nonisolated private static func unused(_ url: URL) -> URL {
        let fm = FileManager.default
        guard fm.fileExists(atPath: url.path) else { return url }
        let base = url.deletingPathExtension().lastPathComponent
        let ext = url.pathExtension
        for n in 2... {
            let candidate = url.deletingLastPathComponent().appending(path: "\(base)-\(n).\(ext)")
            if !fm.fileExists(atPath: candidate.path) { return candidate }
        }
        return url
    }
}

/// At most `limit` thumbnails decode at once: each decodes an embedded JPEG.
actor ThumbnailGate {
    private let limit: Int
    private var running = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []

    init(limit: Int) { self.limit = limit }

    func wait() async {
        if running < limit {
            running += 1
            return
        }
        await withCheckedContinuation { waiting.append($0) }
    }

    func signal() {
        if waiting.isEmpty {
            running -= 1
        } else {
            waiting.removeFirst().resume()
        }
    }
}
