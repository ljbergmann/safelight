import SwiftUI
import UniformTypeIdentifiers

/// The preset library, grouped as the desktop's Presets panel groups it;
/// tapping one applies it to the photo as one History step.
struct PresetsPanel: View {
    let model: LiveModel
    @State private var library: [PresetInfo] = []
    @State private var importing = false
    @State private var expanded = false
    @State private var note: String?

    private var groups: [(String, [PresetInfo])] {
        Dictionary(grouping: library, by: \.group)
            .map { ($0.key, $0.value) }
            .sorted { $0.0 < $1.0 }
    }

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(groups, id: \.0) { group, items in
                    DisclosureGroup(group.isEmpty ? "User Presets" : group) {
                        ForEach(items, id: \.id) { preset in
                            Button(preset.name) { model.live?.applyPreset(id: preset.id); model.refreshSoon() }
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .font(.callout)
                }
                Button("Import Presets…", systemImage: "square.and.arrow.down") { importing = true }
                    .font(.callout)
                if let note { Text(note).font(.caption).foregroundStyle(.secondary) }
            }
            .padding(.top, 4)
        } label: {
            Text("Presets").font(.headline)
        }
        .task { await load() }
        .fileImporter(
            isPresented: $importing,
            allowedContentTypes: [UTType(filenameExtension: "xmp") ?? .data],
            allowsMultipleSelection: true
        ) { result in
            guard case .success(let files) = result else { return }
            let paths = files.compactMap { file -> String? in
                // Copied while the security scope is open.
                guard file.startAccessingSecurityScopedResource() else { return file.path }
                defer { file.stopAccessingSecurityScopedResource() }
                let copy = URL.temporaryDirectory.appending(path: file.lastPathComponent)
                try? FileManager.default.removeItem(at: copy)
                try? FileManager.default.copyItem(at: file, to: copy)
                return copy.path
            }
            let count = (try? importPresets(paths: paths)) ?? 0
            note = "\(count) preset\(count == 1 ? "" : "s") imported"
            Task { await load() }
        }
    }

    private func load() async {
        library = await Task.detached { allPresets() }.value
    }
}

/// Lightroom's Copy Settings: a switch per group under its section; Copy
/// keeps the chosen groups for Paste.
struct CopySettingsSheet: View {
    let model: LiveModel
    @Environment(\.dismiss) private var dismiss
    @State private var groups: [CopyGroup] = copyGroups()
    @State private var chosen: Set<UInt32> = Set(copyGroups().filter(\.defaultOn).map(\.index))

    private var sections: [(String, [CopyGroup])] {
        var order: [String] = []
        var bySection: [String: [CopyGroup]] = [:]
        for g in groups {
            if bySection[g.section] == nil { order.append(g.section) }
            bySection[g.section, default: []].append(g)
        }
        return order.map { ($0, bySection[$0]!) }
    }

    var body: some View {
        NavigationStack {
            List {
                ForEach(sections, id: \.0) { title, items in
                    SwiftUI.Section(title) {
                        ForEach(items, id: \.index) { g in
                            SwiftUI.Toggle(g.label, isOn: Binding(
                                get: { chosen.contains(g.index) },
                                set: { if $0 { chosen.insert(g.index) } else { chosen.remove(g.index) } }
                            ))
                        }
                    }
                }
            }
            .navigationTitle("Copy Settings")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Copy") {
                        model.live?.copySettings(groups: Array(chosen))
                        model.copied = true
                        dismiss()
                    }
                }
                ToolbarItem(placement: .bottomBar) {
                    HStack {
                        Button("Check All") { chosen = Set(groups.map(\.index)) }
                        Button("Check None") { chosen = [] }
                    }
                }
            }
        }
    }
}

extension LiveModel {
    /// Reads the edit back after the worker has had time to apply a preset.
    func refreshSoon() {
        Task { @MainActor in
            for _ in 0..<20 {
                try? await Task.sleep(for: .milliseconds(100))
                refresh()
            }
        }
    }

    func paste() {
        let notes = live?.pasteSettings() ?? []
        pasteNote = notes.isEmpty ? nil : notes.joined(separator: " · ")
        refresh()
    }
}

/// The core's preset list, under a name a view's own properties don't hide.
private func allPresets() -> [PresetInfo] { presets() }
