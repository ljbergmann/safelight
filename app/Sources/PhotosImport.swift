import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

/// The Photos library's own picker, for RAW originals: only what is picked
/// comes to the app, as the original file (`.current`), never a JPEG Photos
/// would make from it. A RAW+JPEG pair's RAW needs read access to the
/// library, asked for only when such a pair is picked.
struct PhotosRawPicker: UIViewControllerRepresentable {
    let done: ([PHPickerResult]) -> Void

    func makeUIViewController(context: Context) -> PHPickerViewController {
        // With the library, results carry their asset's id: a RAW+JPEG pair's
        // RAW is reached through it when the picker offers only the JPEG.
        var configuration = PHPickerConfiguration(photoLibrary: .shared())
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
        /// Picked photos with no RAW original (a JPEG, HEIC or screenshot),
        /// and the kinds of file they were.
        var notRaw = 0
        var notRawKinds: [String] = []
        var failed: [String] = []
    }

    /// Copies the RAW original of each of `results` into `folder`. On the
    /// main actor, where the picker's results are; the copies happen in
    /// Photos' callbacks, off it. The picker hands over the RAW when it is
    /// the photo's own file; a pair Photos imported as RAW+JPEG shows the
    /// JPEG, and its RAW is read from the library instead.
    @MainActor static func stage(_ results: [PHPickerResult], in folder: URL) async -> Staged {
        var staged = Staged()
        var seen: Set<String> = []
        for (i, result) in results.enumerated() {
            let provider = result.itemProvider
            let target = folder.appending(path: String(i))
            let suggested = provider.suggestedName
            let outcome: Result<String, Error>?
            // By the file's own type (CR3, ARW…): loading by the abstract
            // camera-RAW type isn't offered by every provider.
            if let type = provider.registeredTypeIdentifiers
                .compactMap(UTType.init)
                .first(where: { $0.conforms(to: .rawImage) }) {
                outcome = await load(provider, type: type, named: suggested, into: target)
            } else if let id = result.assetIdentifier {
                outcome = await rawResource(of: id, into: target)
            } else {
                outcome = nil
            }
            switch outcome {
            case .success(let path)?: staged.paths.append(path)
            case .failure(let error)?: staged.failed.append("\(suggested ?? "A photo"): \(error.localizedDescription)")
            case nil:
                staged.notRaw += 1
                seen.formUnion(provider.registeredTypeIdentifiers.compactMap { UTType($0)?.preferredFilenameExtension?.uppercased() })
            }
        }
        staged.notRawKinds = seen.sorted()
        return staged
    }

    /// The provider's file of `type`, copied into `target` (the file Photos
    /// hands over lasts only as long as the callback).
    @MainActor private static func load(_ provider: NSItemProvider, type: UTType, named suggested: String?,
                             into target: URL) async -> Result<String, Error> {
        await withCheckedContinuation { done in
            _ = provider.loadFileRepresentation(forTypeIdentifier: type.identifier) { url, error in
                done.resume(returning: Result {
                    guard let url else { throw error ?? CocoaError(.fileReadUnknown) }
                    let name = suggested.map { "\($0).\(url.pathExtension)" } ?? url.lastPathComponent
                    let file = target.appending(path: name)
                    try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
                    try FileManager.default.copyItem(at: url, to: file)
                    return file.path
                })
            }
        }
    }

    /// The RAW resource of the asset `id` (a RAW+JPEG pair's RAW), written
    /// into `target`, downloaded from iCloud if need be; `nil` when the asset
    /// has none. Needs read access to the library, asked for once.
    private static func rawResource(of id: String, into target: URL) async -> Result<String, Error>? {
        let status = await PHPhotoLibrary.requestAuthorization(for: .readWrite)
        guard status == .authorized || status == .limited,
              let asset = PHAsset.fetchAssets(withLocalIdentifiers: [id], options: nil).firstObject,
              let resource = PHAssetResource.assetResources(for: asset).first(where: {
                  UTType($0.uniformTypeIdentifier)?.conforms(to: .rawImage) ?? false
              })
        else { return nil }
        let file = target.appending(path: resource.originalFilename)
        let options = PHAssetResourceRequestOptions()
        options.isNetworkAccessAllowed = true
        do {
            try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
            try await PHAssetResourceManager.default().writeData(for: resource, toFile: file, options: options)
            return .success(file.path)
        } catch {
            return .failure(error)
        }
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
        if staged.notRaw > 0 {
            let kinds = staged.notRawKinds.isEmpty ? "" : " (\(staged.notRawKinds.joined(separator: ", ")))"
            notes.append("\(staged.notRaw) not RAW\(kinds)")
        }
        notes += staged.failed
        finishImport("From Photos: " + (notes.isEmpty ? "nothing imported" : notes.joined(separator: " · ")))
    }

    nonisolated private static func stagingFolder() -> URL {
        FileManager.default.temporaryDirectory.appending(path: "PhotosImport-\(UUID().uuidString)")
    }
}
