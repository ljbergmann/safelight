import SwiftUI
import UIKit

/// Export As…, as Lightroom for iPad lays it out: file type, dimension,
/// quality and the watermark first; naming (with the user's own filename
/// templates), colour space, Metadata, presets and the watermark editor
/// under More options; then the iPadOS share sheet for the files (Save to Photos,
/// Files, AirDrop…). It starts from the last export's choices.
struct ExportSheet: View {
    let library: Library
    let ids: [Int64]
    @Environment(\.dismiss) private var dismiss
    @State private var choice = defaultExportChoice()
    @State private var presets: [ExportPreset] = []
    @State private var naming = false
    @State private var presetName = ""
    @State private var running = false
    @State private var files: [URL] = []
    @State private var problems: [String] = []
    @State private var error: String?
    @State private var templates: [FilenameTemplate] = []
    @State private var watermarks: [WatermarkPreset] = []
    @State private var editingTemplate = false
    @State private var editingWatermarks = false

    var body: some View {
        NavigationStack {
            Form {
                SwiftUI.Section {
                    Picker("File type", selection: $choice.format) {
                        Text("JPG").tag(ExportFormat.jpeg)
                        Text("JPEG XL").tag(ExportFormat.jxl)
                        Text("TIF (16-bit)").tag(ExportFormat.tiff)
                        Text("PNG").tag(ExportFormat.png)
                    }
                    Picker("Dimension", selection: dimension) {
                        Text("Full size").tag(Dimension.full)
                        Text("Small (2048 px)").tag(Dimension.small)
                        Text("Custom").tag(Dimension.custom)
                    }
                    if dimension.wrappedValue == .custom {
                        LabeledContent("Long side") {
                            HStack {
                                TextField("Pixels", value: $choice.longEdge, format: .number.grouping(.never))
                                    .keyboardType(.numberPad)
                                    .multilineTextAlignment(.trailing)
                                    .frame(maxWidth: 90)
                                Text("px").foregroundStyle(Theme.secondaryText)
                            }
                        }
                    }
                    if choice.format == .jpeg || choice.format == .jxl {
                        Picker("Image quality", selection: $choice.quality) {
                            ForEach(Self.qualities, id: \.self) { Text("\($0)%").tag($0) }
                            if !Self.qualities.contains(choice.quality) {
                                Text("\(choice.quality)%").tag(choice.quality)
                            }
                        }
                    }
                }
                SwiftUI.Section {
                    SwiftUI.Toggle("Include watermark", isOn: $choice.watermark)
                    if choice.watermark && !watermarks.isEmpty {
                        Picker("Watermark", selection: $choice.watermarkName) {
                            Text("Simple Copyright").tag("")
                            ForEach(watermarks, id: \.name) { w in Text(w.name).tag(w.name) }
                        }
                    }
                }
                SwiftUI.Section {
                    NavigationLink("More options") { moreOptions }
                }
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
            .navigationTitle("Export as… (\(ids.count))")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", systemImage: "xmark") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Export", systemImage: "checkmark") { export() }
                        .buttonStyle(.borderedProminent)
                        .disabled(running)
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
            .sheet(isPresented: $editingTemplate, onDismiss: reloadExtras) {
                FilenameTemplateEditor(library: library, ids: ids, choice: $choice)
            }
            .sheet(isPresented: $editingWatermarks, onDismiss: reloadExtras) {
                WatermarkEditor(chosen: $choice.watermarkName)
            }
            .onAppear {
                choice = library.lastExportChoice()
                presets = library.exportPresets()
                reloadExtras()
            }
        }
    }

    private static let qualities: [UInt8] = [100, 90, 80, 70, 60, 50]

    private enum Dimension: Hashable { case full, small, custom }

    /// Lightroom's Dimension: full size, a small 2048 px copy, or a long side
    /// of one's own.
    private var dimension: Binding<Dimension> {
        Binding(
            get: {
                switch choice.longEdge {
                case 0: .full
                case 2048: .small
                default: .custom
                }
            },
            set: {
                switch $0 {
                case .full: choice.longEdge = 0
                case .small: choice.longEdge = 2048
                case .custom: if choice.longEdge == 0 || choice.longEdge == 2048 { choice.longEdge = 4096 }
                }
            })
    }

    /// The rest of Lightroom's choices: naming, colour space, metadata,
    /// presets, the editors of templates and watermarks.
    private var moreOptions: some View {
        Form {
            namingSection
            SwiftUI.Section("Color") {
                Picker("Color Space", selection: $choice.colorSpace) {
                    Text("sRGB").tag(ExportColorSpace.srgb)
                    Text("Adobe RGB (1998)").tag(ExportColorSpace.adobeRgb)
                    Text("ProPhoto RGB").tag(ExportColorSpace.proPhoto)
                }
            }
            metadataSection
            SwiftUI.Section("Presets") {
                presetMenu
            }
        }
        .navigationTitle("More options")
    }

    private var namingSection: some View {
        SwiftUI.Section("File Naming") {
            Picker("Template", selection: $choice.template) {
                Text("None").tag("")
                ForEach(templates, id: \.name) { t in Text(t.name).tag(t.template) }
                if !choice.template.isEmpty && !templates.contains(where: { $0.template == choice.template }) {
                    Text(choice.template).tag(choice.template)
                }
            }
            Button("Edit Template…") { editingTemplate = true }
            if choice.template.isEmpty {
                Picker("Rename To", selection: $choice.naming) {
                    Text("Filename").tag(ExportNaming.filename)
                    Text("Filename - Sequence").tag(ExportNaming.filenameSequence)
                    Text("Filename - Custom Text").tag(ExportNaming.filenameText)
                    Text("Custom Name").tag(ExportNaming.customName)
                    Text("Custom Name - Sequence").tag(ExportNaming.customNameSequence)
                    Text("Custom Name (x of y)").tag(ExportNaming.customNameOf)
                    Text("Date - Filename").tag(ExportNaming.dateFilename)
                }
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
            Button("Edit Watermarks…") { editingWatermarks = true }
        }
    }

    private func reloadExtras() {
        templates = (try? filenameTemplates()) ?? []
        watermarks = watermarkPresets()
    }

    /// Custom Text and Sequence apply to a template that has them too.
    private var usesText: Bool {
        if !choice.template.isEmpty { return choice.template.contains("{Custom Text}") }
        return [.filenameText, .customName, .customNameSequence, .customNameOf].contains(choice.naming)
    }

    private var usesSequence: Bool {
        if !choice.template.isEmpty { return choice.template.contains("{Sequence #") }
        return [.filenameSequence, .customNameSequence].contains(choice.naming)
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
