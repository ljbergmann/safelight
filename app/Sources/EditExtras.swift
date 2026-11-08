import SwiftUI

extension LiveModel {
    /// Runs Auto off the main thread; one History step when it lands.
    func runAuto(_ kind: AutoKind) {
        guard let live, autoRunning == nil else { return }
        autoRunning = kind
        Task.detached {
            let outcome = Result { try live.auto(kind: kind) }
            await MainActor.run {
                self.autoRunning = nil
                if case .failure(let error) = outcome { self.pasteNote = "Auto: \(error)" }
                self.refresh()
            }
        }
    }
}

/// History, newest first: tap a step to see the edit as it was then. The
/// later steps stay until a new change replaces them, as in Lightroom.
struct HistoryPanel: View {
    let model: LiveModel
    @State private var expanded = true
    @State private var clearing = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            let steps = model.edit?.steps ?? []
            let applied = Int(model.edit?.applied ?? 0)
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array(steps.enumerated()).reversed(), id: \.offset) { index, step in
                    row(step, at: index + 1, applied: applied)
                }
                row("Opened", at: 0, applied: applied)
            }
            .padding(.top, 4)
        } label: {
            HStack {
                Text("History").font(.headline)
                Spacer()
                Menu("History", systemImage: "ellipsis.circle") {
                    Button("Clear History", systemImage: "trash", role: .destructive) { clearing = true }
                        .disabled((model.edit?.steps.isEmpty ?? true))
                }
                .labelStyle(.iconOnly)
            }
        }
        .confirmationDialog("Clear this photo's History?", isPresented: $clearing, titleVisibility: .visible) {
            Button("Clear History", role: .destructive) { model.live?.clearHistory(); model.refresh() }
        } message: {
            Text("The edit stays as it is; its steps can't be gone back to.")
        }
    }

    private func row(_ title: String, at position: Int, applied: Int) -> some View {
        Button {
            model.live?.goToStep(applied: UInt32(position))
            model.refresh()
        } label: {
            HStack {
                Text(title)
                    .foregroundStyle(position > applied ? .secondary : .primary)
                Spacer()
                if position == applied {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .font(.callout)
        .padding(.vertical, 3)
        .contextMenu {
            Button("Copy History Step Settings to Before", systemImage: "rectangle.lefthalf.inset.filled") {
                model.live?.beforeFromStep(applied: UInt32(position))
            }
        }
    }
}

/// Lightroom's Snapshots, above History: named states of the photo's edit,
/// kept in the catalog. + saves the edit as it is and asks for a name; a tap
/// applies one as a History step; its menu updates, renames or deletes it.
struct SnapshotsPanel: View {
    let model: LiveModel
    @State private var expanded = true
    @State private var list: [SnapshotInfo] = []
    @State private var renaming: SnapshotInfo?
    @State private var name = ""
    @State private var note: String?

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 2) {
                if list.isEmpty {
                    Text("+ saves the photo as it is now").font(.caption).foregroundStyle(.secondary)
                }
                ForEach(list, id: \.id) { snapshot in
                    Button {
                        act { note = try live().applySnapshot(id: snapshot.id).map { "Not rendered: \($0)" }.first }
                        model.refresh()
                    } label: {
                        Text(snapshot.name).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .font(.callout)
                    .padding(.vertical, 3)
                    .contextMenu {
                        Button("Update with Current Settings", systemImage: "arrow.triangle.2.circlepath") {
                            act { try live().updateSnapshot(id: snapshot.id) }
                        }
                        Button("Copy Snapshot Settings to Before", systemImage: "rectangle.lefthalf.inset.filled") {
                            act { note = try live().beforeFromSnapshot(id: snapshot.id).map { "Not rendered: \($0)" }.first }
                        }
                        Button("Rename…", systemImage: "pencil") { name = snapshot.name; renaming = snapshot }
                        Button("Delete", systemImage: "trash", role: .destructive) {
                            act { try live().deleteSnapshot(id: snapshot.id) }
                        }
                    }
                }
                if let note { Text(note).font(.caption).foregroundStyle(.orange) }
            }
            .padding(.top, 4)
        } label: {
            HStack {
                Text("Snapshots").font(.headline)
                Spacer()
                Button("New Snapshot", systemImage: "plus") {
                    act {
                        let id = try live().addSnapshot(name: nil)
                        expanded = true
                        if let made = try live().snapshots().first(where: { $0.id == id }) {
                            name = made.name
                            renaming = made
                        }
                    }
                }
                .labelStyle(.iconOnly)
                .disabled(model.edit == nil)
            }
        }
        .onChange(of: model.photo?.id, initial: true) { load() }
        .onChange(of: model.edit == nil) { load() }
        .alert("Name Snapshot", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Name", text: $name)
            Button("Cancel", role: .cancel) {}
            Button("Save") {
                if let snapshot = renaming { act { try live().renameSnapshot(id: snapshot.id, name: name) } }
            }
        }
    }

    private struct NoPhoto: Error {}

    private func live() throws -> LiveView {
        guard let live = model.live else { throw NoPhoto() }
        return live
    }

    private func act(_ change: () throws -> Void) {
        do {
            note = nil
            try change()
        } catch {
            note = "Snapshot not saved: \(error)"
        }
        load()
    }

    private func load() {
        list = model.live?.snapshots() ?? []
    }
}

/// Above Basic's sliders: Profile, Auto, and white balance's menu and
/// eyedropper.
struct BasicExtras: View {
    @Bindable var model: LiveModel

    var body: some View {
        ProfileMenu(model: model)
        HStack {
            Button {
                model.runAuto(.settings)
            } label: {
                if model.autoRunning == .settings {
                    ProgressView().controlSize(.small)
                } else {
                    Text("Auto")
                }
            }
            .buttonStyle(.bordered)
            .disabled(model.autoRunning != nil)
            Spacer()
        }
        if let edit = model.edit, !edit.whiteBalanceChoices.isEmpty {
            HStack {
                Text("WB").font(.callout)
                Menu {
                    ForEach(edit.whiteBalanceChoices, id: \.self) { choice in
                        Button(choice) {
                            if choice == "Auto" {
                                model.runAuto(.whiteBalance)
                            } else {
                                model.live?.setWhiteBalance(name: choice)
                                model.refresh()
                            }
                        }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text(model.autoRunning == .whiteBalance ? "Auto…" : edit.whiteBalance)
                        Image(systemName: "chevron.up.chevron.down").font(.caption)
                    }
                }
                Spacer()
                Button {
                    model.tool = model.tool == .whiteBalance ? .none : .whiteBalance
                } label: {
                    Image(systemName: "eyedropper")
                }
                .buttonStyle(.bordered)
                .tint(model.tool == .whiteBalance ? .accentColor : .secondary)
            }
            if model.tool == .whiteBalance {
                Text("Tap something that should be neutral grey or white.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
    }
}

/// The point curve: drag the line to add a point, drag a point to move it,
/// double tap a point to remove it. RGB, Red, Green and Blue, and Lightroom's
/// built-in curves.
struct PointCurveEditor: View {
    let model: LiveModel
    @State private var channel: UInt32 = 0
    /// The point a drag is moving, once it has one.
    @State private var dragging: UInt32?
    @State private var curve: CurveState?

    private let colors: [Color] = [.primary, .red, .green, .blue]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Picker("Channel", selection: $channel) {
                    Text("RGB").tag(UInt32(0))
                    Text("R").tag(UInt32(1))
                    Text("G").tag(UInt32(2))
                    Text("B").tag(UInt32(3))
                }
                .pickerStyle(.segmented)
                Menu(curve?.name ?? "Custom") {
                    ForEach(model.live?.builtinCurves() ?? [], id: \.self) { name in
                        Button(name) { model.live?.setBuiltinCurve(name: name); reload() }
                    }
                }
                .font(.callout)
            }
            GeometryReader { geo in
                let size = geo.size
                ZStack {
                    grid(size)
                    if let curve {
                        line(curve, size).stroke(colors[Int(channel)], lineWidth: 1.5)
                        ForEach(0..<curve.points.count / 2, id: \.self) { i in
                            let p = point(curve, i, size)
                            Circle()
                                .fill(dragging == UInt32(i) ? colors[Int(channel)] : Color(.systemBackground))
                                .stroke(colors[Int(channel)], lineWidth: 1.5)
                                .frame(width: 12, height: 12)
                                .position(p)
                                .onTapGesture(count: 2) {
                                    model.live?.removeCurvePoint(channel: channel, index: UInt32(i))
                                    reload()
                                }
                        }
                    }
                }
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag(at: $0.location, in: size, done: false) }
                        .onEnded { drag(at: $0.location, in: size, done: true) }
                )
            }
            .aspectRatio(1, contentMode: .fit)
        }
        .onAppear(perform: reload)
        .onChange(of: channel) { reload() }
        .onChange(of: model.edit?.applied) { reload() }
    }

    private func reload() {
        curve = model.live?.pointCurve(channel: channel)
    }

    private func drag(at location: CGPoint, in size: CGSize, done: Bool) {
        let x = Float(min(max(location.x / size.width, 0), 1))
        let y = Float(min(max(1 - location.y / size.height, 0), 1))
        var index = dragging
        if index == nil, let curve {
            // Starting on a point grabs it; elsewhere adds one.
            let near = (0..<curve.points.count / 2).first { i in
                let p = point(curve, i, size)
                return hypot(p.x - location.x, p.y - location.y) < 22
            }
            index = near.map(UInt32.init)
        }
        dragging = model.live?.setCurvePoint(channel: channel, index: index, x: x, y: y, done: done)
        if done { dragging = nil; model.refresh() }
        reload()
    }

    private func point(_ c: CurveState, _ i: Int, _ size: CGSize) -> CGPoint {
        CGPoint(x: CGFloat(c.points[2 * i]) * size.width, y: (1 - CGFloat(c.points[2 * i + 1])) * size.height)
    }

    private func line(_ c: CurveState, _ size: CGSize) -> Path {
        Path { path in
            for (i, v) in c.samples.enumerated() {
                let p = CGPoint(x: CGFloat(i) / CGFloat(c.samples.count - 1) * size.width,
                                y: (1 - CGFloat(v)) * size.height)
                if i == 0 { path.move(to: p) } else { path.addLine(to: p) }
            }
        }
    }

    private func grid(_ size: CGSize) -> some View {
        Path { path in
            for k in 1..<4 {
                let f = CGFloat(k) / 4
                path.move(to: CGPoint(x: f * size.width, y: 0)); path.addLine(to: CGPoint(x: f * size.width, y: size.height))
                path.move(to: CGPoint(x: 0, y: f * size.height)); path.addLine(to: CGPoint(x: size.width, y: f * size.height))
            }
            path.move(to: CGPoint(x: 0, y: size.height)); path.addLine(to: CGPoint(x: size.width, y: 0))
        }
        .stroke(Color.secondary.opacity(0.3), lineWidth: 0.5)
        .background(Color.secondary.opacity(0.08))
    }
}

/// The Profile browser: camera profiles and Adobe's at the top, each group of
/// looks in a submenu, as on the desktop. Profile Amount is a Basic slider.
struct ProfileMenu: View {
    let model: LiveModel

    var body: some View {
        let profiles = model.edit == nil ? [] : (model.live?.profiles() ?? [])
        let top = profiles.filter { $0.group.isEmpty }
        let groups = Array(Set(profiles.map(\.group).filter { !$0.isEmpty }))
            .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
        HStack {
            Text("Profile").font(.callout)
            Menu {
                ForEach(top, id: \.title) { item($0) }
                ForEach(groups, id: \.self) { group in
                    Menu(group) {
                        ForEach(profiles.filter { $0.group == group }, id: \.title) { item($0) }
                    }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(profiles.first(where: \.selected)?.title ?? "—").lineLimit(1)
                    Image(systemName: "chevron.up.chevron.down").font(.caption)
                }
            }
            Spacer()
        }
    }

    private func item(_ p: ProfileInfo) -> some View {
        Button {
            model.live?.setProfile(index: p.index)
            model.refresh()
        } label: {
            if p.selected { Label(p.title, systemImage: "checkmark") } else { Text(p.title) }
        }
    }
}

/// The histogram of what the view shows, as on the desktop, with Lightroom's
/// triangles: lit when shadows or highlights clip, tapped to show them on the
/// photo (blue and red).
struct HistogramPanel: View {
    let model: LiveModel
    @State private var showShadows = false
    @State private var showHighlights = false

    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.25)) { _ in
            if let h = model.live?.histogram() {
                ZStack(alignment: .top) {
                    Canvas { context, size in
                        let peak = max(1, ([h.red, h.green, h.blue].flatMap { $0.dropFirst().dropLast() }.max() ?? 1))
                        for (bins, color) in [(h.red, Color.red), (h.green, Color.green), (h.blue, Color.blue)] {
                            var path = Path()
                            path.move(to: CGPoint(x: 0, y: size.height))
                            for (i, n) in bins.enumerated() {
                                let x = CGFloat(i) / 255 * size.width
                                let y = size.height * (1 - min(1, CGFloat(n) / CGFloat(peak)))
                                path.addLine(to: CGPoint(x: x, y: y))
                            }
                            path.addLine(to: CGPoint(x: size.width, y: size.height))
                            context.fill(path, with: .color(color.opacity(0.45)))
                        }
                    }
                    .blendMode(.plusLighter)
                    .background(Color.black.opacity(0.85))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    HStack {
                        triangle(lit: h.shadowsClipped, on: $showShadows, systemImage: "arrowtriangle.up.fill", color: .blue)
                        Spacer()
                        triangle(lit: h.highlightsClipped, on: $showHighlights, systemImage: "arrowtriangle.up.fill", color: .red)
                    }
                    .padding(4)
                }
                .frame(height: 90)
            }
        }
        .onChange(of: showShadows) { model.live?.setClipping(shadows: showShadows, highlights: showHighlights) }
        .onChange(of: showHighlights) { model.live?.setClipping(shadows: showShadows, highlights: showHighlights) }
    }

    private func triangle(lit: Bool, on: Binding<Bool>, systemImage: String, color: Color) -> some View {
        Button { on.wrappedValue.toggle() } label: {
            Image(systemName: systemImage)
                .font(.caption)
                .foregroundStyle(on.wrappedValue ? color : (lit ? .white : .gray))
                .padding(4)
                .background(on.wrappedValue ? Color.white.opacity(0.2) : .clear, in: RoundedRectangle(cornerRadius: 3))
        }
        .buttonStyle(.plain)
    }
}
