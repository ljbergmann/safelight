import SwiftUI

/// What a touch on the photo does in the Heal pane.
enum HealKind: Hashable {
    case heal, clone, redEye, petEye

    var isEye: Bool { self == .redEye || self == .petEye }
}

extension LiveModel {
    /// Places what the Heal pane's tool places, from a touch on the photo: a
    /// circle where it tapped, a brushed spot along a drag, or an eye's
    /// correction. `points` are view points (fractions across the photo).
    func heal(along points: [[Float]], moved: Bool) {
        guard let live, let last = points.last else { return }
        healNote = nil
        switch healKind {
        case .redEye, .petEye:
            do {
                selectedEye = try live.addRedEye(at: last, size: eyeSize, pet: healKind == .petEye)
            } catch RenderError.Failed(let message) {
                healNote = message
            } catch {
                healNote = "\(error)"
            }
        case .heal, .clone:
            let along = moved ? points.flatMap { $0 } : last
            selectedSpot = live.addSpot(points: along, radius: spotSize, clone: healKind == .clone,
                                        feather: spotFeather, opacity: spotOpacity)
            if selectedSpot == nil { healNote = "Too many spots on this photo" }
        }
        refresh()
    }
}

/// The Heal pane: Heal, Clone, Red Eye and Pet Eye, the new spot's or the
/// selected one's settings, and Visualize Spots.
struct HealPane: View {
    @Bindable var model: LiveModel
    /// A slider being dragged and its value, shown until the drag ends.
    @State private var dragging: (String, Float)?

    var body: some View {
        // Read for SwiftUI to redraw after each change.
        let _ = model.edit?.applied
        let _ = model.edit?.steps.count
        VStack(alignment: .leading, spacing: 14) {
            Text("Heal").font(.title3.weight(.semibold))
            Picker("Tool", selection: Binding(get: { model.healKind }, set: { choose($0) })) {
                Text("Heal").tag(HealKind.heal)
                Text("Clone").tag(HealKind.clone)
                Text("Red Eye").tag(HealKind.redEye)
                Text("Pet Eye").tag(HealKind.petEye)
            }
            .pickerStyle(.segmented)
            if model.healKind.isEye {
                eyeSettings
            } else {
                spotSettings
            }
            if let note = model.healNote {
                Text(note).font(.callout).foregroundStyle(.orange)
            }
        }
        .onAppear { model.tool = .heal }
        .onDisappear {
            if model.tool == .heal { model.tool = .none }
            model.selectedSpot = nil
            model.selectedEye = nil
            model.visualizeSpots = false
        }
    }

    /// Heal or Clone: a selected spot changes to it, as the desktop's menu
    /// does; otherwise new spots get it.
    private func choose(_ kind: HealKind) {
        if !kind.isEye, !model.healKind.isEye, let i = model.selectedSpot, let spot = selectedSpot(i) {
            model.live?.setSpot(index: i, radius: spot.radius, feather: spot.feather, opacity: spot.opacity,
                                clone: kind == .clone, done: true)
            model.refresh()
        } else {
            model.selectedSpot = nil
        }
        model.selectedEye = nil
        // The eye tools have no Visualize Spots switch to turn it off with.
        if kind.isEye { model.visualizeSpots = false }
        model.healKind = kind
        model.healNote = nil
    }

    private func selectedSpot(_ i: UInt32) -> SpotInfo? {
        model.live?.spots().first { $0.index == i }
    }

    @ViewBuilder private var spotSettings: some View {
        if let i = model.selectedSpot, let spot = selectedSpot(i) {
            let set = { (radius: Float, feather: Float, opacity: Float, done: Bool) in
                model.live?.setSpot(index: i, radius: radius, feather: feather, opacity: opacity,
                                    clone: spot.clone, done: done)
                if done { model.refresh() }
            }
            row("Size", spot.radius, 0.005...0.1, shown: { "\(Int(($0 * 1000).rounded()))" }) { v, done in
                set(v, spot.feather, spot.opacity, done)
            }
            row("Feather", spot.feather, 0...1, shown: percent) { v, done in set(spot.radius, v, spot.opacity, done) }
            row("Opacity", spot.opacity, 0...1, shown: percent) { v, done in set(spot.radius, spot.feather, v, done) }
            HStack {
                Button("Next Source", systemImage: "shuffle") {
                    if model.live?.nextSpotSource(index: i) != true { model.healNote = "No other source fits" }
                    model.refresh()
                }
                Spacer()
                Button("Delete", systemImage: "trash", role: .destructive) {
                    model.live?.deleteSpot(index: i)
                    model.selectedSpot = nil
                    model.refresh()
                }
                Button("Done") { model.selectedSpot = nil }.buttonStyle(.borderedProminent)
            }
            .font(.callout)
            Text("Drag the spot, or the circle it copies from, to move it.")
                .font(.caption).foregroundStyle(Theme.secondaryText)
        } else {
            row("Size", model.spotSize, 0.005...0.1, shown: { "\(Int(($0 * 1000).rounded()))" }) { v, _ in model.spotSize = v }
            row("Feather", model.spotFeather, 0...1, shown: percent) { v, _ in model.spotFeather = v }
            row("Opacity", model.spotOpacity, 0...1, shown: percent) { v, _ in model.spotOpacity = v }
            Text("Tap a spot on the photo to \(model.healKind == .clone ? "clone over" : "heal") it, or paint over a larger area. Tap a pin to change its spot.")
                .font(.callout).foregroundStyle(Theme.secondaryText)
            Text("\(model.edit?.spots ?? 0) spot\(model.edit?.spots == 1 ? "" : "s") on this photo")
                .font(.caption).foregroundStyle(Theme.secondaryText)
        }
        SwiftUI.Toggle("Visualize Spots", isOn: $model.visualizeSpots)
        if model.visualizeSpots {
            row("Threshold", model.visualizeThreshold, 0...1, shown: percent) { v, _ in model.visualizeThreshold = v }
        }
    }

    @ViewBuilder private var eyeSettings: some View {
        if let i = model.selectedEye, let eye = model.live?.redEyes().first(where: { $0.index == i }) {
            let set = { (pupil: Float, darken: Float, done: Bool) in
                model.live?.setRedEye(index: i, pupilSize: pupil, darken: darken, done: done)
                if done { model.refresh() }
            }
            row("Pupil Size", eye.pupilSize, 0...1, shown: percent) { v, done in set(v, eye.darken, done) }
            row("Darken", eye.darken, 0...1, shown: percent) { v, done in set(eye.pupilSize, v, done) }
            HStack {
                Spacer()
                Button("Delete", systemImage: "trash", role: .destructive) {
                    model.live?.deleteRedEye(index: i)
                    model.selectedEye = nil
                    model.refresh()
                }
                Button("Done") { model.selectedEye = nil }.buttonStyle(.borderedProminent)
            }
            .font(.callout)
        } else {
            row("Size", model.eyeSize, 0.01...0.2, shown: { "\(Int(($0 * 1000).rounded()))" }) { v, _ in model.eyeSize = v }
            Text("Tap the middle of \(model.healKind == .petEye ? "an animal's" : "a") \(model.healKind == .petEye ? "glowing" : "red") eye, with the whole eye within the circle's size.")
                .font(.callout).foregroundStyle(Theme.secondaryText)
        }
    }

    private func percent(_ v: Float) -> String { "\(Int((v * 100).rounded()))" }

    private func row(_ label: String, _ value: Float, _ range: ClosedRange<Float>, shown: @escaping (Float) -> String,
                     changed: @escaping (Float, Bool) -> Void) -> some View {
        let current = dragging?.0 == label ? dragging!.1 : value
        return VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(label).foregroundStyle(Theme.secondaryText)
                Spacer()
                Text(shown(current)).monospacedDigit()
            }
            ThinSlider(value: current, range: range) { v, done in
                dragging = done ? nil : (label, v)
                changed(v, done)
            }
        }
    }
}

/// Over the photo while the Heal pane is open: Visualize Spots, each spot's
/// pin (tap to select, drag to move), the selected spot's outline and its
/// source's, and the eye corrections' outlines.
struct HealOnPhoto: View {
    let model: LiveModel
    let size: CGSize
    @State private var visualized: UIImage?
    /// The last point of a drag, as a view point, and whether it moved.
    @State private var last: [Float]?
    @State private var moved = false
    /// Bumped while dragging, to redraw from the core's positions.
    @State private var tick = 0

    var body: some View {
        let _ = model.edit?.applied
        let _ = model.edit?.steps.count
        let _ = tick
        let rect = model.photoRect(in: size)
        let at = { (x: Float, y: Float) in
            CGPoint(x: rect.minX + CGFloat(x) * rect.width, y: rect.minY + CGFloat(y) * rect.height)
        }
        let long = max(rect.width, rect.height)
        ZStack(alignment: .topLeading) {
            if model.visualizeSpots, let visualized {
                Image(uiImage: visualized)
                    .resizable()
                    .frame(width: rect.width, height: rect.height)
                    .offset(x: rect.minX, y: rect.minY)
                    .allowsHitTesting(false)
            }
            if model.healKind.isEye {
                ForEach(model.live?.redEyes() ?? [], id: \.index) { eye in
                    let selected = model.selectedEye == eye.index
                    Path { p in
                        let points = stride(from: 0, to: eye.outline.count - 1, by: 2).map { at(eye.outline[$0], eye.outline[$0 + 1]) }
                        p.addLines(points)
                        p.closeSubpath()
                    }
                    .stroke(.white, lineWidth: selected ? 2 : 1)
                    .shadow(color: .black.opacity(0.6), radius: 1)
                    .allowsHitTesting(false)
                    pin(at: at(eye.center[0], eye.center[1]), selected: selected) { from, to, done in
                        model.live?.dragRedEye(index: eye.index, from: from, to: to, done: done)
                    } tapped: {
                        model.selectedEye = selected ? nil : eye.index
                    }
                }
            } else {
                ForEach(model.live?.spots() ?? [], id: \.index) { spot in
                    let selected = model.selectedSpot == spot.index
                    let pinAt = at(spot.pin[0], spot.pin[1])
                    let sourceAt = at(spot.source[0], spot.source[1])
                    let r = CGFloat(spot.radius) * long
                    if selected {
                        outline(spot, r: r, shift: .zero, at: at)
                            .stroke(.white, lineWidth: 2)
                        outline(spot, r: r, shift: CGPoint(x: sourceAt.x - pinAt.x, y: sourceAt.y - pinAt.y), at: at)
                            .stroke(.white, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
                        Path { p in p.move(to: sourceAt); p.addLine(to: pinAt) }
                            .stroke(.white.opacity(0.8), lineWidth: 1)
                            .allowsHitTesting(false)
                        pin(at: sourceAt, selected: false, small: true) { from, to, done in
                            model.live?.dragSpot(index: spot.index, from: from, to: to, source: true, done: done)
                        } tapped: {}
                    }
                    pin(at: pinAt, selected: selected) { from, to, done in
                        model.live?.dragSpot(index: spot.index, from: from, to: to, source: false, done: done)
                    } tapped: {
                        model.selectedSpot = selected ? nil : spot.index
                    }
                }
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .task(id: "\(model.visualizeSpots)-\(model.visualizeThreshold)-\(model.edit?.applied ?? 0)-\(model.edit?.steps.count ?? 0)") {
            await visualize()
        }
    }

    /// A spot's outline: its circle, or the outline of its brushed path, moved
    /// by `shift` points (to draw its source).
    private func outline(_ spot: SpotInfo, r: CGFloat, shift: CGPoint, at: (Float, Float) -> CGPoint) -> some Shape {
        Path { p in
            if spot.brushed {
                let points = stride(from: 0, to: spot.path.count - 1, by: 2).map {
                    let q = at(spot.path[$0], spot.path[$0 + 1])
                    return CGPoint(x: q.x + shift.x, y: q.y + shift.y)
                }
                var line = Path()
                line.addLines(points)
                p.addPath(line.strokedPath(StrokeStyle(lineWidth: 2 * r, lineCap: .round, lineJoin: .round)))
            } else {
                let c = at(spot.pin[0], spot.pin[1])
                p.addEllipse(in: CGRect(x: c.x + shift.x - r, y: c.y + shift.y - r, width: 2 * r, height: 2 * r))
            }
        }
    }

    /// A pin: tapped, or dragged (each step from the last point; `done` ends it).
    private func pin(at point: CGPoint, selected: Bool, small: Bool = false,
                     drag: @escaping ([Float], [Float], Bool) -> Void,
                     tapped: @escaping () -> Void) -> some View {
        let d: CGFloat = small ? 10 : 16
        let rect = model.photoRect(in: size)
        return Circle()
            .fill(selected ? Theme.accent : .white.opacity(0.001))
            .overlay(Circle().strokeBorder(.white, lineWidth: 2))
            .shadow(color: .black.opacity(0.6), radius: 1.5)
            .frame(width: d, height: d)
            .padding(10)
            .contentShape(Circle())
            .position(point)
            .gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .local)
                    .onChanged { g in
                        if hypot(g.translation.width, g.translation.height) > 4 { moved = true }
                        guard moved else { return }
                        let to = model.viewPoint(g.location, in: rect)
                        drag(last ?? model.viewPoint(g.startLocation, in: rect), to, false)
                        last = to
                        tick += 1
                    }
                    .onEnded { g in
                        if moved {
                            let to = model.viewPoint(g.location, in: rect)
                            drag(last ?? to, to, true)
                            model.refresh()
                        } else {
                            tapped()
                        }
                        last = nil
                        moved = false
                    }
            )
    }

    private func visualize() async {
        guard model.visualizeSpots, let live = model.live else {
            visualized = nil
            return
        }
        try? await Task.sleep(for: .milliseconds(150))
        if Task.isCancelled { return }
        let threshold = model.visualizeThreshold
        let made = await Task.detached { live.visualizeSpots(threshold: threshold, maxEdge: 1280) }.value
        guard let made, !Task.isCancelled else { return }
        visualized = Self.grey(made)
    }

    private static func grey(_ o: MaskOverlay) -> UIImage? {
        let (w, h) = (Int(o.width), Int(o.height))
        guard let provider = CGDataProvider(data: Data(o.alpha) as CFData),
              let cg = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: w,
                               space: CGColorSpaceCreateDeviceGray(),
                               bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
                               provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
}
