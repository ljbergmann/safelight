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
    @State private var expanded = false

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
            Text("History").font(.headline)
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
