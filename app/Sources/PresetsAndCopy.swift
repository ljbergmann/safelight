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

    /// The group open, by name; `nil`, the list of groups.
    @State private var group: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Presets").font(.title3.weight(.semibold))
                Spacer()
                Menu {
                    Button("Create Preset…", systemImage: "plus") { creating = true }
                    Button("Import Presets…", systemImage: "square.and.arrow.down") { importing = true }
                } label: {
                    Image(systemName: "plus").padding(8)
                        .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
            }
            .padding(.bottom, 12)
            if let amount = model.edit?.presetAmount { amountSlider(amount).padding(.bottom, 12) }
            if let note { Text(note).font(.caption).foregroundStyle(Theme.secondaryText).padding(.bottom, 8) }
            if let group, let items = groups.first(where: { $0.0 == group })?.1 {
                Button {
                    withAnimation(.snappy(duration: 0.2)) { self.group = nil }
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: "chevron.left").font(.footnote.weight(.semibold))
                        Text(Self.name(of: group)).font(.headline)
                        Spacer()
                    }
                    .padding(.vertical, 10).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Rectangle().fill(Theme.line).frame(height: 0.5)
                TimelineView(.periodic(from: .now, by: 0.3)) { _ in
                    VStack(spacing: 2) {
                        ForEach(items, id: \.id) { preset in row(preset) }
                    }
                }
                .padding(.top, 6)
                .task(id: group) {
                    model.live?.requestPresetPreviews(ids: items.map(\.id), edge: 128)
                }
            } else {
                ForEach(groups, id: \.0) { name, items in
                    Button {
                        withAnimation(.snappy(duration: 0.2)) { group = name }
                    } label: {
                        HStack(spacing: 12) {
                            IconTile(systemImage: "camera.filters")
                            VStack(alignment: .leading, spacing: 1) {
                                Text(Self.name(of: name))
                                Text("\(items.count) preset\(items.count == 1 ? "" : "s")")
                                    .font(.caption).foregroundStyle(Theme.secondaryText)
                            }
                            Spacer()
                            Image(systemName: "chevron.right").font(.footnote).foregroundStyle(Theme.secondaryText)
                        }
                        .padding(.vertical, 6).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
            }
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
            allowedContentTypes: [UTType(filenameExtension: "xmp") ?? .data, .folder],
            allowsMultipleSelection: true
        ) { result in
            guard case .success(let picks) = result, !picks.isEmpty else { return }
            let live = model.live
            Task {
                // Imported while the files' security scope is open; a pack's
                // looks join the profiles, so the photo lists them again.
                let summary = await Task.detached { () -> ImportSummary in
                    let opened = picks.filter { $0.startAccessingSecurityScopedResource() }
                    defer { opened.forEach { $0.stopAccessingSecurityScopedResource() } }
                    let summary = importPresets(paths: picks.map(\.path))
                    if summary.done > 0 { live?.reloadProfiles() }
                    return summary
                }.value
                note = ([summary.message] + summary.details.prefix(3)).joined(separator: "\n")
                await load()
            }
        }
    }

    private static func name(of group: String) -> String { group.isEmpty ? "User Presets" : group }

    /// A preset: its preview on this photo, its name, and for one made here
    /// or imported, a menu; a tap applies it.
    private func row(_ preset: PresetInfo) -> some View {
        HStack(spacing: 12) {
            Button {
                model.live?.applyPreset(id: preset.id)
                model.refreshSoon()
            } label: {
                HStack(spacing: 12) {
                    Group {
                        if let data = model.live?.presetPreview(id: preset.id), let image = UIImage(data: data) {
                            Image(uiImage: image).resizable().scaledToFill()
                        } else {
                            Theme.chosen
                        }
                    }
                    .frame(width: 56, height: 56)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    Text(preset.name).lineLimit(2)
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if preset.managed {
                Menu {
                    Button("Update with Current Settings", systemImage: "arrow.triangle.2.circlepath") {
                        act("Updated \(preset.name)") { try model.live?.updatePreset(id: preset.id) }
                    }
                    Button("Rename…", systemImage: "pencil") { newName = preset.name; renaming = preset }
                    Button("Delete", systemImage: "trash", role: .destructive) { deleting = preset }
                } label: {
                    Image(systemName: "ellipsis").frame(width: 32, height: 32)
                }
            }
        }
        .padding(.vertical, 3)
    }

    private func amountSlider(_ amount: PresetAmountState) -> some View {
        let shown = dragging ?? amount.amount
        return VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Amount · \(amount.name)").font(.callout).lineLimit(1)
                Spacer()
                Text("\(Int((shown * 100).rounded()))").font(.callout.monospacedDigit()).foregroundStyle(.secondary)
            }
            ThinSlider(value: shown, range: 0...2, reset: resetAmount) { value, done in
                if done {
                    model.live?.setPresetAmount(amount: value, done: true)
                    dragging = nil
                    model.refresh()
                } else {
                    dragging = value
                    model.live?.setPresetAmount(amount: value, done: false)
                }
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { resetAmount() }
    }

    /// A double tap: back to the preset as applied, 100%.
    private func resetAmount() {
        model.live?.setPresetAmount(amount: 1, done: true)
        model.refresh()
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
