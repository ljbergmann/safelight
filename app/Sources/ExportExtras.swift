import SwiftUI
import UniformTypeIdentifiers

/// Lightroom's Filename Template Editor: the template as text, tokens in
/// braces (`{Filename}`) inserted from their menus, an example of the
/// first photo's name, and saving it by name. Done uses it for this export.
struct FilenameTemplateEditor: View {
    let library: Library
    let ids: [Int64]
    @Binding var choice: ExportChoice
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var name = ""
    @State private var templates: [FilenameTemplate] = []
    @State private var said: String?
    private let menus = filenameTokenMenus()

    var body: some View {
        NavigationStack {
            Form {
                SwiftUI.Section {
                    TextField("Template", text: $text, axis: .vertical)
                        .font(.body.monospaced())
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                    HStack {
                        ForEach(menus, id: \.name) { menu in
                            Menu(menu.name) {
                                ForEach(menu.tokens, id: \.self) { token in
                                    Button(token.trimmingCharacters(in: CharacterSet(charactersIn: "{}"))) { text += token }
                                }
                            }
                        }
                    }
                    .font(.callout)
                    if let problem = filenameTemplateProblem(template: text) {
                        Text(problem).font(.callout).foregroundStyle(.red)
                    } else if let id = ids.first {
                        LabeledContent("Example", value: example(id))
                            .foregroundStyle(Theme.secondaryText)
                    }
                } header: {
                    Text("Template")
                } footer: {
                    Text(verbatim: "Type text, and insert tokens from the menus. Type \\{ for a brace.")
                }
                SwiftUI.Section("Saved Templates") {
                    ForEach(templates, id: \.name) { t in
                        Button {
                            text = t.template
                            name = t.name
                        } label: {
                            LabeledContent(t.name, value: t.template)
                        }
                        .swipeActions {
                            Button("Delete", role: .destructive) {
                                run { try deleteFilenameTemplate(name: t.name) }
                            }
                        }
                    }
                    HStack {
                        TextField("Name", text: $name)
                        Button(templates.contains { $0.name == name } ? "Update" : "Save") {
                            run { try saveFilenameTemplate(name: name, template: text) }
                        }
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty
                                  || filenameTemplateProblem(template: text) != nil)
                    }
                    if let said { Text(said).font(.callout).foregroundStyle(.red) }
                }
            }
            .navigationTitle("Filename Template")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Use") {
                        choice.template = text
                        dismiss()
                    }
                    .disabled(filenameTemplateProblem(template: text) != nil)
                }
            }
            .onAppear {
                text = choice.template.isEmpty ? "{Filename}" : choice.template
                templates = (try? filenameTemplates()) ?? []
                name = templates.first { $0.template == text }?.name ?? ""
            }
        }
    }

    private func example(_ id: Int64) -> String {
        var c = choice
        c.template = text
        return library.exportExampleName(choice: c, id: id, total: UInt32(ids.count))
    }

    private func run(_ change: () throws -> Void) {
        do {
            try change()
            said = nil
        } catch RenderError.Failed(let message) {
            said = message
        } catch {
            said = "\(error)"
        }
        templates = (try? filenameTemplates()) ?? []
    }
}

/// Watermark presets, as Lightroom's Watermark Editor makes them: text or
/// a graphic (a PNG or JPEG of the user's), its colour, shadow, opacity,
/// size, one of nine anchor points, inset and rotation. Saved where
/// RAWmakase keeps them; the chosen one marks this export.
struct WatermarkEditor: View {
    @Binding var chosen: String
    @Environment(\.dismiss) private var dismiss
    @State private var presets: [WatermarkPreset] = []
    @State private var editing: WatermarkPreset?
    @State private var original: String?
    @State private var image: URL?
    @State private var picking = false
    @State private var said: String?

    var body: some View {
        NavigationStack {
            Form {
                if let shown = editing {
                    editor(Binding(get: { editing ?? shown }, set: { editing = $0 }))
                } else {
                    SwiftUI.Section {
                        ForEach(presets, id: \.name) { p in
                            Button {
                                editing = p
                                original = p.name
                                image = nil
                            } label: {
                                LabeledContent(p.name, value: p.graphic ? "Graphic" : p.text)
                            }
                            .swipeActions {
                                Button("Delete", role: .destructive) {
                                    run { try deleteWatermark(name: p.name) }
                                    if chosen == p.name { chosen = "" }
                                }
                            }
                        }
                        Button("New Watermark", systemImage: "plus") {
                            var w = newWatermark()
                            w.name = ""
                            editing = w
                            original = nil
                            image = nil
                        }
                    } footer: {
                        Text("The Simple Copyright Watermark needs no preset: it marks each photo with its own copyright.")
                    }
                }
                if let said { Text(said).foregroundStyle(.red) }
            }
            .navigationTitle(editing == nil ? "Watermarks" : (original ?? "New Watermark"))
            .toolbar {
                if editing != nil {
                    ToolbarItem(placement: .cancellationAction) { Button("Back") { editing = nil; said = nil } }
                    ToolbarItem(placement: .confirmationAction) { Button("Save") { save() } }
                } else {
                    ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                }
            }
            .fileImporter(isPresented: $picking, allowedContentTypes: [.png, .jpeg]) { result in
                if case .success(let url) = result { image = url }
            }
            .onAppear { presets = watermarkPresets() }
        }
    }

    @ViewBuilder private func editor(_ w: Binding<WatermarkPreset>) -> some View {
        SwiftUI.Section {
            TextField("Name", text: w.name)
            Picker("Style", selection: w.graphic) {
                Text("Text").tag(false)
                Text("Graphic").tag(true)
            }
            .pickerStyle(.segmented)
            if w.wrappedValue.graphic {
                Button(image?.lastPathComponent ?? (w.wrappedValue.hasImage ? "Replace Image…" : "Choose Image…")) { picking = true }
            } else {
                TextField("Text", text: w.text)
                ColorPicker("Color", selection: Binding(
                    get: {
                        let c = w.wrappedValue.color
                        return Color(red: Double(c[0]), green: Double(c[1]), blue: Double(c[2]))
                    },
                    set: { color in
                        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
                        UIColor(color).getRed(&r, green: &g, blue: &b, alpha: &a)
                        w.wrappedValue.color = [Float(r), Float(g), Float(b)].map { min(max($0, 0), 1) }
                    }), supportsOpacity: false)
                SwiftUI.Toggle("Shadow", isOn: w.shadow)
            }
        }
        SwiftUI.Section("Effects") {
            slider("Opacity", w.opacity, 0.05...1)
            slider("Size", w.size, 0.02...1)
            slider("Inset", w.inset, 0...0.25)
            LabeledContent("Anchor") {
                Grid(horizontalSpacing: 6, verticalSpacing: 6) {
                    ForEach(0..<3) { row in
                        GridRow {
                            ForEach(0..<3) { column in
                                let on = w.wrappedValue.anchorX == UInt8(column) && w.wrappedValue.anchorY == UInt8(row)
                                Button {
                                    w.wrappedValue.anchorX = UInt8(column)
                                    w.wrappedValue.anchorY = UInt8(row)
                                } label: {
                                    Circle().strokeBorder(Theme.secondaryText, lineWidth: 1)
                                        .background(Circle().fill(on ? Theme.accent : .clear))
                                        .frame(width: 18, height: 18)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
            }
            Picker("Rotation", selection: w.rotation) {
                Text("0°").tag(UInt8(0))
                Text("90° left").tag(UInt8(1))
                Text("180°").tag(UInt8(2))
                Text("90° right").tag(UInt8(3))
            }
        }
    }

    private func slider(_ label: String, _ value: Binding<Float>, _ range: ClosedRange<Float>) -> some View {
        LabeledContent("\(label) \(Int((value.wrappedValue * 100).rounded()))%") {
            Slider(value: value, in: range)
        }
    }

    private func save() {
        guard let w = editing else { return }
        let picked = image
        let reading = picked?.startAccessingSecurityScopedResource() ?? false
        defer { if reading { picked?.stopAccessingSecurityScopedResource() } }
        do {
            let saved = try saveWatermark(preset: w, replacing: original, image: picked?.path)
            if chosen == original { chosen = saved.name }
            editing = nil
            said = nil
        } catch RenderError.Failed(let message) {
            said = message
        } catch {
            said = "\(error)"
        }
        presets = watermarkPresets()
    }

    private func run(_ change: () throws -> Void) {
        do {
            try change()
            said = nil
        } catch RenderError.Failed(let message) {
            said = message
        } catch {
            said = "\(error)"
        }
        presets = watermarkPresets()
    }
}
