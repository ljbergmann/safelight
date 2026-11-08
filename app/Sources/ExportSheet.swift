import SwiftUI
import UIKit

/// Export: format, size and quality, then the iPadOS share sheet for the
/// files (Save to Photos, Files, AirDrop…).
struct ExportSheet: View {
    let library: Library
    let ids: [Int64]
    @Environment(\.dismiss) private var dismiss
    @State private var format: ExportFormat = .jpeg
    @State private var quality: Double = 90
    @State private var longEdge: UInt32 = 0
    @State private var running = false
    @State private var files: [URL] = []
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                Picker("Format", selection: $format) {
                    Text("JPEG").tag(ExportFormat.jpeg)
                    Text("TIFF (16-bit)").tag(ExportFormat.tiff)
                    Text("PNG").tag(ExportFormat.png)
                }
                if format == .jpeg {
                    LabeledContent("Quality \(Int(quality))") {
                        Slider(value: $quality, in: 30...100, step: 1)
                    }
                }
                Picker("Size", selection: $longEdge) {
                    Text("Full size").tag(UInt32(0))
                    Text("Long edge 4096").tag(UInt32(4096))
                    Text("Long edge 2048").tag(UInt32(2048))
                    Text("Long edge 1080").tag(UInt32(1080))
                }
                if let error { Text(error).foregroundStyle(.red) }
                if running {
                    HStack { ProgressView(); Text("Exporting \(ids.count) photo\(ids.count == 1 ? "" : "s")…") }
                }
            }
            .navigationTitle("Export")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Export") { export() }.disabled(running)
                }
            }
            .sheet(isPresented: Binding(get: { !files.isEmpty }, set: { if !$0 { files = []; dismiss() } })) {
                ShareSheet(items: files)
            }
        }
    }

    private func export() {
        running = true
        error = nil
        let choice = ExportChoice(format: format, quality: UInt8(quality), longEdge: longEdge)
        let folder = URL.temporaryDirectory.appending(path: "Export-\(UUID().uuidString)")
        let (library, ids) = (library, ids)
        Task.detached {
            let outcome = Result { try library.exportPhotos(ids: ids, choice: choice, folder: folder.path) }
            await MainActor.run {
                running = false
                switch outcome {
                case .success(let paths): files = paths.map { URL(fileURLWithPath: $0) }
                case .failure(let e): error = "\(e)"
                }
            }
        }
    }
}

/// UIKit's share sheet.
struct ShareSheet: UIViewControllerRepresentable {
    let items: [URL]
    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }
    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
