import SwiftUI
import UniformTypeIdentifiers

/// The preset library, grouped as the desktop's Presets panel groups it;
/// tapping one applies it to the photo as one History step. After a preset
/// that offers one, its Amount shows at the top, as in Lightroom, until
/// anything else changes the photo. Presets made here or imported have a
/// menu: Update with Current Settings, Rename, Delete.
struct PresetsPanel: View {
    let model: LiveModel
    @State private var library: [PresetInfo] = []
    @State private var importing = false
    @State private var creating = false
    @State private var expanded = false
    @State private var note: String?
    @State private var renaming: PresetInfo?
    @State private var newName = ""
    @State private var deleting: PresetInfo?
    /// The Amount being dragged, shown instead of the core's until it ends.
    @State private var dragging: Float?

    private var groups: [(String, [PresetInfo])] {
        Dictionary(grouping: library, by: \.group)
            .map { ($0.key, $0.value) }
            .sorted { $0.0 < $1.0 }
    }

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 6) {
                if let amount = model.edit?.presetAmount { amountSlider(amount) }
                ForEach(groups, id: \.0) { group, items in
                    DisclosureGroup(group.isEmpty ? "User Presets" : group) {
                        ForEach(items, id: \.id) { preset in row(preset) }
                    }
                    .font(.callout)
                }
                HStack {
                    Button("Create Preset…", systemImage: "plus") { creating = true }
                    Button("Import Presets…", systemImage: "square.and.arrow.down") { importing = true }
                }
                .font(.callout)
                if let note { Text(note).font(.caption).foregroundStyle(.secondary) }
            }
            .padding(.top, 4)
        } label: {
            Text("Presets").font(.headline)
        }
        .task { await load() }
        .sheet(isPresented: $creating, onDismiss: { Task { await load() } }) {
            NewPresetSheet(model: model) { note = $0 }
        }
        .alert("Rename Preset", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Name", text: $newName)
            Button("Cancel", role: .cancel) {}
            Button("Rename") {
                if let preset = renaming { act("Renamed") { try renamePreset(id: preset.id, name: newName) } }
            }
        }
        .confirmationDialog(
            "Delete \(deleting?.name ?? "")?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                if let preset = deleting { act("Deleted") { try deletePreset(id: preset.id) } }
            }
        }
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

    private func row(_ preset: PresetInfo) -> some View {
        Button(preset.name) { model.live?.applyPreset(id: preset.id); model.refreshSoon() }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contextMenu {
                if preset.managed {
                    Button("Update with Current Settings", systemImage: "arrow.triangle.2.circlepath") {
                        act("Updated \(preset.name)") { try model.live?.updatePreset(id: preset.id) }
                    }
                    Button("Rename…", systemImage: "pencil") { newName = preset.name; renaming = preset }
                    Button("Delete", systemImage: "trash", role: .destructive) { deleting = preset }
                }
            }
    }

    private func amountSlider(_ amount: PresetAmountState) -> some View {
        let shown = dragging ?? amount.amount
        return VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Amount · \(amount.name)").font(.callout).lineLimit(1)
                Spacer()
                Text("\(Int((shown * 100).rounded()))").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
            }
            Slider(
                value: Binding(
                    get: { shown },
                    set: { value in
                        dragging = value
                        model.live?.setPresetAmount(amount: value, done: false)
                    }
                ),
                in: 0...2
            ) { editing in
                if !editing {
                    model.live?.setPresetAmount(amount: shown, done: true)
                    dragging = nil
                    model.refresh()
                }
            }
        }
    }

    /// Runs a change to the preset library, says how it went and lists the
    /// presets again.
    private func act(_ done: String, _ change: () throws -> Void) {
        do {
            try change()
            note = done
        } catch {
            note = "Not changed: \(error)"
        }
        Task { await load() }
    }

    private func load() async {
        library = await Task.detached { allPresets() }.value
    }
}

/// Lightroom's Create Preset: a name, a group, and the settings it holds,
/// chosen as in Copy Settings.
struct NewPresetSheet: View {
    let model: LiveModel
    let done: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var group = "User Presets"
    @State private var newGroup = ""
    @State private var groups: [String] = presetGroups()
    @State private var chosen: Set<UInt32> = Set(copyGroups().filter(\.defaultOn).map(\.index))
    @State private var error: String?

    var body: some View {
        NavigationStack {
            SettingGroupsList(chosen: $chosen) {
                SwiftUI.Section {
                    TextField("Preset Name", text: $name)
                    Picker("Group", selection: $group) {
                        ForEach(groups, id: \.self) { Text($0).tag($0) }
                        Text("New Group…").tag("")
                    }
                    if group.isEmpty { TextField("New Group Name", text: $newGroup) }
                    if let error { Text(error).foregroundStyle(.red).font(.caption) }
                }
            }
            .navigationTitle("Create Preset")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") {
                        do {
                            _ = try model.live?.createPreset(
                                name: name, group: group.isEmpty ? newGroup : group, groups: Array(chosen))
                            done("Created \(name)")
                            dismiss()
                        } catch {
                            self.error = "\(error)"
                        }
                    }
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || chosen.isEmpty)
                }
            }
        }
    }
}

/// The setting groups of Copy Settings and Create Preset: a switch per group
/// under its section, with Check All and Check None.
struct SettingGroupsList<Header: View>: View {
    @Binding var chosen: Set<UInt32>
    @ViewBuilder var header: Header
    private let groups = copyGroups()

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
        List {
            header
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
        .toolbar {
            ToolbarItem(placement: .bottomBar) {
                HStack {
                    Button("Check All") { chosen = Set(groups.map(\.index)) }
                    Button("Check None") { chosen = [] }
                }
            }
        }
    }
}

/// Lightroom's Copy Settings: a switch per group under its section; Copy
/// keeps the chosen groups for Paste.
struct CopySettingsSheet: View {
    let model: LiveModel
    @Environment(\.dismiss) private var dismiss
    @State private var chosen: Set<UInt32> = Set(copyGroups().filter(\.defaultOn).map(\.index))

    var body: some View {
        NavigationStack {
            SettingGroupsList(chosen: $chosen) { EmptyView() }
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
