import Photos
import SwiftUI

/// Lightroom's Save to Device and Share: the photos exported with the last
/// Export As… settings, then saved to the Photos library or handed to the
/// share sheet, without the dialog.
@MainActor @Observable
final class QuickExport {
    enum Goal { case save, share }

    var running = false
    /// What the share sheet shows.
    var files: [URL] = []
    /// What happened, shown briefly over the photo.
    var said: String?

    func run(_ goal: Goal, library: Library, ids: [Int64]) {
        guard !running else { return }
        running = true
        said = nil
        let folder = URL.temporaryDirectory.appending(path: "Export-\(UUID().uuidString)")
        Task {
            let outcome = await Task.detached {
                Result { try library.exportPhotos(ids: ids, choice: library.lastExportChoice(), folder: folder.path) }
            }.value
            switch outcome {
            case .success(let result):
                let urls = result.files.map { URL(fileURLWithPath: $0) }
                switch goal {
                case .share: files = urls
                case .save: said = await Self.save(urls)
                }
                if let problem = result.problems.first { said = problem }
            case .failure(RenderError.Failed(let message)): said = message
            case .failure(let error): said = "\(error)"
            }
            running = false
            if said != nil {
                try? await Task.sleep(for: .seconds(3))
                said = nil
            }
        }
    }

    /// Adds the files to the Photos library (asking once for permission to
    /// add); says how it went. Not on the main actor: Photos runs the change
    /// block on its own queue.
    nonisolated private static func save(_ urls: [URL]) async -> String {
        let status = await PHPhotoLibrary.requestAuthorization(for: .addOnly)
        guard status == .authorized || status == .limited else {
            return "Safelight may not add to Photos: allow it in Settings › Privacy › Photos"
        }
        do {
            try await PHPhotoLibrary.shared().performChanges {
                for url in urls {
                    PHAssetCreationRequest.forAsset().addResource(with: .photo, fileURL: url, options: nil)
                }
            }
            return urls.count == 1 ? "Saved to Photos" : "Saved \(urls.count) photos to Photos"
        } catch {
            return "Not saved to Photos: \(error.localizedDescription)"
        }
    }
}
