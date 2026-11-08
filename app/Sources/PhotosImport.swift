import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// The Photos library's own picker, for RAW originals. It needs no access to
/// the library: only what is picked comes to the app, and as the original
/// file (`.current`), never a JPEG Photos would make from it.
struct PhotosRawPicker: UIViewControllerRepresentable {
    let done: ([PHPickerResult]) -> Void

    func makeUIViewController(context: Context) -> PHPickerViewController {
        var configuration = PHPickerConfiguration()
        configuration.filter = .images
        configuration.selectionLimit = 0
        configuration.selection = .ordered
        configuration.preferredAssetRepresentationMode = .current
        let picker = PHPickerViewController(configuration: configuration)
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ picker: PHPickerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator { Coordinator(done: done) }

    final class Coordinator: NSObject, PHPickerViewControllerDelegate {
        let done: ([PHPickerResult]) -> Void
        init(done: @escaping ([PHPickerResult]) -> Void) { self.done = done }

        func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
            done(results)
        }
    }
}

/// Photos' originals copied out for the import, each into a folder of its own
/// (two photos can share a name), which the import then moves into the
/// catalog; the app removes the whole folder afterwards.
enum PhotosStaging {
    struct Staged {
        var paths: [String] = []
        /// Picked photos with no RAW original (a JPEG, HEIC or screenshot).
        var notRaw = 0
        var failed: [String] = []
    }

    /// Copies the RAW original of each of `results` into `folder`. On the
    /// main actor, where the picker's results are; the copies happen in
    /// Photos' callbacks, off it.
    @MainActor static func stage(_ results: [PHPickerResult], in folder: URL) async -> Staged {
        var staged = Staged()
        let raw = UTType.rawImage.identifier
        for (i, result) in results.enumerated() {
            let provider = result.itemProvider
            guard provider.hasItemConformingToTypeIdentifier(raw) else {
                staged.notRaw += 1
                continue
            }
            let target = folder.appending(path: String(i))
            let suggested = provider.suggestedName
            // The file Photos hands over lasts only as long as the callback:
            // it is copied there.
            let outcome: Result<String, Error> = await withCheckedContinuation { done in
                _ = provider.loadFileRepresentation(forTypeIdentifier: raw) { url, error in
                    done.resume(with: .success(Result {
                        guard let url else { throw error ?? CocoaError(.fileReadUnknown) }
                        let name = suggested.map { "\($0).\(url.pathExtension)" } ?? url.lastPathComponent
                        let file = target.appending(path: name)
                        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
                        try FileManager.default.copyItem(at: url, to: file)
                        return file.path
                    }))
                }
            }
            switch outcome {
            case .success(let path): staged.paths.append(path)
            case .failure(let error): staged.failed.append("\(suggested ?? "A photo"): \(error.localizedDescription)")
            }
        }
        return staged
    }
}

extension LibraryModel {
    /// Imports the RAW originals of photos picked in Photos: copied out,
    /// then moved into their dated folders, duplicates of photos the
    /// catalog has left out. Says what came in and what didn't.
    func importFromPhotos(_ results: [PHPickerResult]) async {
        guard !results.isEmpty else { return }
        setImporting("Getting \(results.count) originals from Photos…")
        let folder = Self.stagingFolder()
        let staged = await PhotosStaging.stage(results, in: folder)
        await importStaged(staged, from: folder)
    }

    /// `-photos-import-test`: stages a copy of the catalog's first photo as
    /// the picker would and imports it, which must find it already there and
    /// leave nothing staged. Prints `PHOTOS IMPORT …`.
    func photosImportTest() async {
        guard let photo = photos.first else { print("PHOTOS IMPORT no photo to stage"); return }
        let folder = Self.stagingFolder()
        let target = folder.appending(path: "0")
        var staged = PhotosStaging.Staged()
        do {
            try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
            let file = target.appending(path: photo.filename)
            try FileManager.default.copyItem(at: URL(fileURLWithPath: photo.path), to: file)
            staged.paths = [file.path]
        } catch {
            staged.failed = ["\(error)"]
        }
        let before = photos.count
        await importStaged(staged, from: folder)
        print("PHOTOS IMPORT \(lastImport ?? "?"); photos \(before) -> \(photos.count); staging left \(FileManager.default.fileExists(atPath: folder.path))")
    }

    private func importStaged(_ staged: PhotosStaging.Staged, from folder: URL) async {
        defer { try? FileManager.default.removeItem(at: folder) }
        var notes: [String] = []
        if !staged.paths.isEmpty {
            notes += await runImport(staged.paths, mode: .move)
        } else {
            setImporting(nil)
        }
        if staged.notRaw > 0 { notes.append("\(staged.notRaw) not RAW") }
        notes += staged.failed
        finishImport("From Photos: " + (notes.isEmpty ? "nothing imported" : notes.joined(separator: " · ")))
    }

    nonisolated private static func stagingFolder() -> URL {
        FileManager.default.temporaryDirectory.appending(path: "PhotosImport-\(UUID().uuidString)")
    }
}
