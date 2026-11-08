import SwiftUI
import UIKit

/// Export, as Lightroom's dialog: presets, File Naming with an example,
/// format, colour space and size, Metadata and the Simple Copyright
/// Watermark; then the iPadOS share sheet for the files (Save to Photos,
/// Files, AirDrop…). It starts from the last export's choices.
struct ExportSheet: View {
    let library: Library
    let ids: [Int64]
    @Environment(\.dismiss) private var dismiss
    @State private var choice = ExportChoice(format: .jpeg, quality: 90, longEdge: 0, colorSpace: .srgb,
                                             naming: .filename, customText: "", startNumber: 1,
                                             metadata: .all, removeLocation: false, watermark: false)
    @State private var presets: [ExportPreset] = []
    @State private var naming = false
    @State private var presetName = ""
    @State private var running = false
    @State private var files: [URL] = []
    @State private var problems: [String] = []
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                namingSection
                fileSection
                metadataSection
                if let error { Text(error).foregroundStyle(.red) }
                if !problems.isEmpty {
                    SwiftUI.Section("Not everything went as planned") {
                        ForEach(problems, id: \.self) { Text($0).font(.callout) }
                    }
                }
                if running {
                    HStack { ProgressView(); Text("Exporting \(ids.count) photo\(ids.count == 1 ? "" : "s")…") }
                }
            }
            .navigationTitle("Export")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .principal) { presetMenu }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Export") { export() }.disabled(running)
                }
            }
            .alert("Save Preset", isPresented: $naming) {
                TextField("Name", text: $presetName)
                Button("Save") { savePreset() }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("A preset of the same name is replaced.")
            }
            .sheet(isPresented: Binding(get: { !files.isEmpty }, set: { if !$0 { files = []; if problems.isEmpty { dismiss() } } })) {
                ShareSheet(items: files)
            }
            .onAppear {
                choice = library.lastExportChoice()
                presets = library.exportPresets()
            }
        }
    }

    private var namingSection: some View {
        SwiftUI.Section("File Naming") {
            Picker("Rename To", selection: $choice.naming) {
                Text("Filename").tag(ExportNaming.filename)
                Text("Filename - Sequence").tag(ExportNaming.filenameSequence)
                Text("Filename - Custom Text").tag(ExportNaming.filenameText)
                Text("Custom Name").tag(ExportNaming.customName)
                Text("Custom Name - Sequence").tag(ExportNaming.customNameSequence)
                Text("Custom Name (x of y)").tag(ExportNaming.customNameOf)
                Text("Date - Filename").tag(ExportNaming.dateFilename)
            }
            if usesText {
                TextField("Custom Text", text: $choice.customText)
            }
            if usesSequence {
                Stepper("Start Number \(choice.startNumber)", value: $choice.startNumber, in: 1...99999)
            }
            LabeledContent("Example", value: ids.first.map { library.exportExampleName(choice: choice, id: $0, total: UInt32(ids.count)) } ?? "")
                .foregroundStyle(Theme.secondaryText)
        }
    }

    private var fileSection: some View {
        SwiftUI.Section("File Settings") {
            Picker("Format", selection: $choice.format) {
                Text("JPEG").tag(ExportFormat.jpeg)
                Text("JPEG XL").tag(ExportFormat.jxl)
                Text("TIFF (16-bit)").tag(ExportFormat.tiff)
                Text("PNG").tag(ExportFormat.png)
            }
            if choice.format == .jpeg || choice.format == .jxl {
                LabeledContent("Quality \(choice.quality)") {
                    Slider(value: Binding(get: { Double(choice.quality) }, set: { choice.quality = UInt8($0) }),
                           in: 30...100, step: 1)
                }
            }
            Picker("Color Space", selection: $choice.colorSpace) {
                Text("sRGB").tag(ExportColorSpace.srgb)
                Text("Adobe RGB (1998)").tag(ExportColorSpace.adobeRgb)
                Text("ProPhoto RGB").tag(ExportColorSpace.proPhoto)
            }
            Picker("Size", selection: $choice.longEdge) {
                Text("Full size").tag(UInt32(0))
                Text("Long edge 4096").tag(UInt32(4096))
                Text("Long edge 2048").tag(UInt32(2048))
                Text("Long edge 1080").tag(UInt32(1080))
                if ![0, 4096, 2048, 1080].contains(choice.longEdge) {
                    Text("Long edge \(choice.longEdge)").tag(choice.longEdge)
                }
            }
        }
    }

    private var metadataSection: some View {
        SwiftUI.Section("Metadata") {
            Picker("Include", selection: $choice.metadata) {
                Text("Copyright Only").tag(ExportMetadata.copyrightOnly)
                Text("Copyright & Contact Info Only").tag(ExportMetadata.copyrightAndContact)
                Text("All Except Camera & Camera Raw Info").tag(ExportMetadata.allExceptCameraAndCameraRaw)
                Text("All Except Camera Raw Info").tag(ExportMetadata.allExceptCameraRaw)
                Text("All Metadata").tag(ExportMetadata.all)
            }
            // Lightroom always leaves the location out of the copyright ones.
            if choice.metadata != .copyrightOnly && choice.metadata != .copyrightAndContact {
                SwiftUI.Toggle("Remove Location Info", isOn: $choice.removeLocation)
            }
            SwiftUI.Toggle("Simple Copyright Watermark", isOn: $choice.watermark)
        }
    }

    private var usesText: Bool {
        [.filenameText, .customName, .customNameSequence, .customNameOf].contains(choice.naming)
    }

    private var usesSequence: Bool {
        [.filenameSequence, .customNameSequence].contains(choice.naming)
    }

    private var presetMenu: some View {
        Menu {
            ForEach(presets, id: \.name) { preset in
                Button(preset.name) { choice = preset.choice }
            }
            if !presets.isEmpty { Divider() }
            Button("Save Current Settings as Preset…", systemImage: "plus") {
                presetName = ""
                naming = true
            }
            if !presets.isEmpty {
                Menu("Delete Preset", systemImage: "trash") {
                    ForEach(presets, id: \.name) { preset in
                        Button(preset.name, role: .destructive) {
                            do {
                                try library.deleteExportPreset(name: preset.name)
                            } catch {
                                self.error = "\(error)"
                            }
                            presets = library.exportPresets()
                        }
                    }
                }
            }
        } label: {
            Label("Presets", systemImage: "list.bullet")
        }
    }

    private func savePreset() {
        do {
            try library.saveExportPreset(name: presetName, choice: choice)
        } catch RenderError.Failed(let message) {
            error = message
        } catch {
            self.error = "\(error)"
        }
        presets = library.exportPresets()
    }

    private func export() {
        running = true
        error = nil
        problems = []
        let folder = URL.temporaryDirectory.appending(path: "Export-\(UUID().uuidString)")
        let (library, ids, choice) = (library, ids, choice)
        Task.detached {
            let outcome = Result { try library.exportPhotos(ids: ids, choice: choice, folder: folder.path) }
            await MainActor.run {
                running = false
                switch outcome {
                case .success(let result):
                    problems = result.problems
                    files = result.files.map { URL(fileURLWithPath: $0) }
                case .failure(RenderError.Failed(let message)): error = message
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
