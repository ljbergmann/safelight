import SwiftUI

/// The Color Mixer's eight bands, red to magenta, as their swatches show them.
enum MixerBand {
    static let names = ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"]
    static let colors: [Color] = [
        Color(red: 0.93, green: 0.2, blue: 0.2), Color(red: 0.95, green: 0.55, blue: 0.15),
        Color(red: 0.95, green: 0.85, blue: 0.15), Color(red: 0.2, green: 0.7, blue: 0.25),
        Color(red: 0.15, green: 0.8, blue: 0.85), Color(red: 0.25, green: 0.4, blue: 0.95),
        Color(red: 0.6, green: 0.25, blue: 0.9), Color(red: 0.9, green: 0.2, blue: 0.75),
    ]
}

/// The Color Mixer as Lightroom on the iPad shows it: a swatch per band, and
/// the chosen band's Hue, Saturation and Luminance.
struct ColorMixView: View {
    /// "Color Mixer · Hue", "· Saturation" and "· Luminance", each a slider per band.
    let sections: [Section]
    let model: LiveModel
    @State private var band = 0
    /// Every band at once rather than one at a time.
    @State private var all = false
    @State private var pointColor = ProcessInfo.processInfo.arguments.contains { $0.hasPrefix("-point-color") }

    private static let channels = ["Hue", "Saturation", "Luminance"]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(sections.first?.switches ?? [], id: \.label) { SwitchRow(item: $0, model: model) }
            Picker("View", selection: $pointColor) {
                Text("Mixer").tag(false)
                Text("Point Color").tag(true)
            }
            .pickerStyle(.segmented)
            if pointColor {
                PointColorView(model: model)
            } else {
                HStack {
                    Text("Drag on the photo").font(.caption).foregroundStyle(Theme.secondaryText)
                    Spacer()
                    // Which of the band's sliders the drag moves.
                    Menu {
                        ForEach([TargetKind.hue, .saturation, .luminance], id: \.self) { kind in
                            Button {
                                model.targeted = model.targeted == kind ? nil : kind
                                model.targetedNote = nil
                            } label: {
                                let name = kind == .hue ? "Hue" : kind == .saturation ? "Saturation" : "Luminance"
                                if model.targeted == kind { Label(name, systemImage: "checkmark") } else { Text(name) }
                            }
                        }
                        if model.targeted != nil { Button("Off") { model.targeted = nil } }
                    } label: {
                        Image(systemName: "scope").foregroundStyle(.white).padding(6)
                            .background([TargetKind.hue, .saturation, .luminance].contains(model.targeted ?? .toneCurve) ? Theme.accent : Theme.chosen,
                                        in: RoundedRectangle(cornerRadius: 8))
                    }
                    .accessibilityLabel("Targeted adjustment")
                }
                TargetedHint(model: model)
                mixer
            }
        }
        .onChange(of: pointColor) {
            if !pointColor, model.tool == .pointColor { model.tool = .none }
            if pointColor, [TargetKind.hue, .saturation, .luminance].contains(model.targeted ?? .toneCurve) { model.targeted = nil }
        }
    }

    @ViewBuilder private var mixer: some View {
            Picker("Bands", selection: $all) {
                Text("By Color").tag(false)
                Text("All").tag(true)
            }
            .pickerStyle(.segmented)
            if all {
                // Every band's Hue, Saturation and Luminance at once, as the
                // desktop's All view.
                ForEach(Array(Self.channels.enumerated()), id: \.offset) { channel, name in
                    Text(name).font(.subheadline.weight(.semibold)).padding(.top, 6)
                    ForEach(MixerBand.names.indices, id: \.self) { b in
                        if let slider = slider(channel, band: b) {
                            SliderRow(slider: slider, model: model, track: track(channel, band: b), label: MixerBand.names[b])
                        }
                    }
                }
            } else {
                byColor
            }
    }

    @ViewBuilder private var byColor: some View {
            HStack(spacing: 0) {
                ForEach(MixerBand.colors.indices, id: \.self) { index in
                    swatch(index)
                    if index < MixerBand.colors.count - 1 { Spacer(minLength: 0) }
                }
            }
            .padding(.vertical, 4)
            ForEach(Array(Self.channels.enumerated()), id: \.offset) { channel, name in
                if let slider = slider(channel) {
                    SliderRow(slider: slider, model: model, track: track(channel), label: name)
                }
            }
    }

    private func slider(_ channel: Int, band: Int? = nil) -> SliderState? {
        let band = band ?? self.band
        let title = "Color Mixer · \(Self.channels[channel])"
        guard let sliders = sections.first(where: { $0.title == title })?.sliders, band < sliders.count else { return nil }
        return sliders[band]
    }

    private func swatch(_ index: Int) -> some View {
        let color = MixerBand.colors[index]
        let edited = (0..<3).contains { channel in
            guard let s = sections.first(where: { $0.title == "Color Mixer · \(Self.channels[channel])" })?.sliders,
                  index < s.count else { return false }
            return abs(s[index].value - s[index].reset) > 0.0001
        }
        return Button { band = index } label: {
            ZStack {
                Circle().fill(index == band ? color : .clear)
                Circle().strokeBorder(color, lineWidth: 2.5)
                if index == band {
                    Circle().fill(.white).frame(width: 7, height: 7)
                } else if edited {
                    // A band moved off its start, as a dot under the ring.
                    Circle().fill(color).frame(width: 5, height: 5).offset(y: 20)
                }
            }
            .frame(width: 28, height: 28)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(MixerBand.names[index])
        .accessibilityAddTraits(index == band ? .isSelected : [])
    }

    /// Hue between the band's neighbours; saturation from grey to the band;
    /// luminance from dark to light.
    private func track(_ channel: Int, band: Int? = nil) -> [Color] {
        let band = band ?? self.band
        let colors = MixerBand.colors
        let color = colors[band]
        switch channel {
        case 0: return [colors[(band + colors.count - 1) % colors.count], color, colors[(band + 1) % colors.count]]
        case 1: return [Color(white: 0.5), color]
        default: return [Color(white: 0.1), color, Color(white: 0.95)]
        }
    }
}

/// Color Grading as its own pane, as Lightroom on the iPad opens it: the
/// region (Shadows, Midtones, Highlights, Global), its wheel and sliders,
/// then Blending and Balance.
struct ColorGradingPane: View {
    let model: LiveModel
    let close: () -> Void
    /// The region shown large, or the 3-Way view of all three tonal
    /// ranges (`threeWay`), the desktop's default.
    @State private var region = 0
    @State private var threeWay = true

    private static let regions = ["Shadows", "Midtones", "Highlights", "Global"]

    var body: some View {
        let sections = (model.edit?.sections ?? []).filter { $0.title.hasPrefix("Color Grading") }
        let grade = threeWay ? nil : sections.first { $0.title == "Color Grading · \(Self.regions[region])" }
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Color Grading").font(.title3.weight(.semibold))
                Spacer()
                Button(action: close) { Image(systemName: "xmark").font(.body.weight(.semibold)) }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Close Color Grading")
            }
            .padding(16)
            Rectangle().fill(Theme.line).frame(height: 0.5)
            VStack(alignment: .leading, spacing: 12) {
                ForEach(sections.first?.switches ?? [], id: \.label) { SwitchRow(item: $0, model: model) }
                regionPicker
                if threeWay {
                    // Midtones above, Shadows and Highlights below, each with its
                    // Luminance, as the desktop's 3-Way view.
                    VStack(spacing: 10) {
                        smallWheel(1, sections)
                        HStack(alignment: .top, spacing: 12) {
                            smallWheel(0, sections)
                            smallWheel(2, sections)
                        }
                    }
                    .frame(maxWidth: .infinity)
                } else if let grade, grade.sliders.count == 3 {
                    GradingWheel(
                        hue: grade.sliders[0].value, saturation: grade.sliders[1].value,
                        set: { x, y, done in
                            model.live?.setGradeWheel(region: UInt32(region), x: x, y: y, done: done)
                            model.refresh()
                        },
                        reset: {
                            model.live?.resetGradeWheel(region: UInt32(region))
                            model.refresh()
                        })
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                    Text(Self.regions[region]).font(.subheadline.weight(.semibold))
                    let hue = Color(hue: Double(grade.sliders[0].value), saturation: 1, brightness: 1)
                    SliderRow(slider: grade.sliders[0], model: model,
                              track: [.red, .yellow, .green, .cyan, .blue, .purple, .red], label: "Hue")
                    SliderRow(slider: grade.sliders[1], model: model, track: [Color(white: 0.5), hue], label: "Saturation")
                    SliderRow(slider: grade.sliders[2], model: model, track: [Color(white: 0.1), Color(white: 0.95)], label: "Luminance")
                }
                Rectangle().fill(Theme.line).frame(height: 0.5).padding(.vertical, 4)
                ForEach(sections.first { $0.title == "Color Grading · Blending" }?.sliders ?? [], id: \.label) {
                    SliderRow(slider: $0, model: model)
                }
            }
            .padding(16)
        }
    }

    /// One region's wheel, small, its name and its Luminance, for the 3-Way view.
    @ViewBuilder private func smallWheel(_ index: Int, _ sections: [Section]) -> some View {
        if let grade = sections.first(where: { $0.title == "Color Grading · \(Self.regions[index])" }), grade.sliders.count == 3 {
            VStack(spacing: 4) {
                GradingWheel(
                    hue: grade.sliders[0].value, saturation: grade.sliders[1].value,
                    set: { x, y, done in
                        model.live?.setGradeWheel(region: UInt32(index), x: x, y: y, done: done)
                        model.refresh()
                    },
                    reset: {
                        model.live?.resetGradeWheel(region: UInt32(index))
                        model.refresh()
                    },
                    size: 120)
                Text(Self.regions[index]).font(.caption).foregroundStyle(Theme.secondaryText)
                SliderRow(slider: grade.sliders[2], model: model, track: [Color(white: 0.1), Color(white: 0.95)], label: "Luminance")
            }
            .frame(maxWidth: 200)
        }
    }

    /// 3-Way, then Shadows, Midtones and Highlights as dark to light dots,
    /// then Global.
    private var regionPicker: some View {
        HStack(spacing: 22) {
            Button { threeWay = true } label: {
                VStack(spacing: 5) {
                    Image(systemName: "circle.grid.3x3.circle").resizable()
                        .foregroundStyle(Color(white: 0.85))
                        .frame(width: 24, height: 24).padding(3)
                        .overlay(Circle().strokeBorder(threeWay ? Theme.accent : .clear, lineWidth: 2))
                    Circle().fill(threeWay ? Theme.accent : .clear).frame(width: 4, height: 4)
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel("3-Way")
            .accessibilityAddTraits(threeWay ? .isSelected : [])
            Rectangle().fill(Theme.line).frame(width: 1, height: 18)
            ForEach(0..<3) { index in
                regionButton(index) {
                    Circle().fill(Color(white: [0.15, 0.5, 0.9][index]))
                        .overlay(Circle().strokeBorder(Color(white: 0.6), lineWidth: 1))
                }
            }
            Rectangle().fill(Theme.line).frame(width: 1, height: 18)
            regionButton(3) {
                Image(systemName: "circle.lefthalf.filled").resizable().foregroundStyle(Color(white: 0.85))
            }
        }
        .frame(maxWidth: .infinity)
    }

    private func regionButton<Icon: View>(_ index: Int, @ViewBuilder icon: () -> Icon) -> some View {
        let chosen = !threeWay && index == region
        return Button { region = index; threeWay = false } label: {
            VStack(spacing: 5) {
                icon()
                    .frame(width: 24, height: 24)
                    .padding(3)
                    .overlay(Circle().strokeBorder(chosen ? Theme.accent : .clear, lineWidth: 2))
                Circle().fill(chosen ? Theme.accent : .clear).frame(width: 4, height: 4)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Self.regions[index])
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

/// A color wheel: hue round it (red at the right, turning counter-clockwise),
/// saturation out from the centre. Dragging sets both; the core rounds them
/// and names the History step. A double tap takes the color off.
struct GradingWheel: View {
    let hue: Float
    let saturation: Float
    /// The point dragged to, in wheel radii from the centre with y down.
    let set: (Float, Float, Bool) -> Void
    /// A double tap: no color.
    let reset: () -> Void

    /// Across, in points: large on its own, smaller in the 3-Way view.
    var size: CGFloat = 220

    var body: some View {
        let radius = size / 2
        let angle = Double(hue) * 2 * .pi
        let puck = CGPoint(x: radius + CGFloat(cos(angle)) * CGFloat(saturation) * radius,
                           y: radius - CGFloat(sin(angle)) * CGFloat(saturation) * radius)
        ZStack {
            // Clockwise on screen is hue going down.
            Circle().fill(AngularGradient(
                colors: stride(from: 0.0, through: 1.0, by: 1.0 / 12).map { Color(hue: (1 - $0).truncatingRemainder(dividingBy: 1), saturation: 1, brightness: 1) },
                center: .center))
            Circle().fill(RadialGradient(colors: [Color(white: 0.85), Color(white: 0.85, opacity: 0)], center: .center, startRadius: 0, endRadius: radius))
            Circle()
                .strokeBorder(.white, lineWidth: 2)
                .background(Circle().fill(.black.opacity(0.25)))
                .frame(width: 22, height: 22)
                .position(puck)
            Circle().fill(.white).frame(width: 4, height: 4).position(puck)
        }
        .frame(width: size, height: size)
        .contentShape(Circle())
        // A few points of travel before a drag starts, so a double tap
        // doesn't move the puck first.
        .gesture(DragGesture(minimumDistance: 4)
            .onChanged { drag in let p = point(drag.location, radius); set(p.0, p.1, false) }
            .onEnded { drag in let p = point(drag.location, radius); set(p.0, p.1, true) })
        .simultaneousGesture(TapGesture(count: 2).onEnded { reset() })
        .accessibilityElement()
        .accessibilityLabel("Color wheel")
        .accessibilityValue("Hue \(Int((hue * 360).rounded()))°, saturation \(Int((saturation * 100).rounded()))")
    }

    private func point(_ location: CGPoint, _ radius: CGFloat) -> (Float, Float) {
        (Float((location.x - radius) / radius), Float((location.y - radius) / radius))
    }
}

/// Point Color: the eyedropper, up to eight swatches, and the chosen
/// swatch's sampled colour beside what it becomes, then its sliders. The
/// eyedropper samples through [`SampleLoupe`].
struct PointColorView: View {
    let model: LiveModel
    @State private var visualize = ProcessInfo.processInfo.arguments.contains("-point-color-ranges")
    @State private var showRanges = ProcessInfo.processInfo.arguments.contains("-point-color-ranges")

    private static let slots = 8

    var body: some View {
        // Read again as the edit or a drag changes: `live` itself never does.
        let _ = model.dragging?.1
        let swatches = model.edit == nil ? [] : (model.live?.pointColors() ?? [])
        let chosen = swatches.first { $0.index == model.pointColorSelected } ?? swatches.last
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Button {
                    model.tool = model.tool == .pointColor ? .none : .pointColor
                    model.pointColorNote = nil
                } label: {
                    Image(systemName: "eyedropper").padding(8)
                        .background(model.tool == .pointColor ? Theme.accent : Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Sample a colour")
                ForEach(0..<Self.slots, id: \.self) { slot in
                    if slot < swatches.count {
                        swatch(swatches[slot], chosen: swatches[slot].index == chosen?.index)
                    } else {
                        Circle().strokeBorder(Theme.line, lineWidth: 1).frame(width: 22, height: 22)
                    }
                }
            }
            if let note = model.pointColorNote {
                Text(note).font(.caption).foregroundStyle(Theme.secondaryText)
            } else if model.tool == .pointColor {
                Text("Move the circle onto a colour, then tap ✓.").font(.caption).foregroundStyle(Theme.secondaryText)
            } else if swatches.isEmpty {
                Text("Tap the eyedropper to sample a colour in the photo.").font(.caption).foregroundStyle(Theme.secondaryText)
            }
            if let chosen {
                HStack(spacing: 0) {
                    Self.color(chosen.color)
                    Self.color(chosen.adjusted)
                }
                .frame(height: 18)
                .clipShape(RoundedRectangle(cornerRadius: 4))
                .accessibilityLabel("The sampled colour, and what it becomes")
                ForEach(chosen.sliders, id: \.label) { SliderRow(slider: $0, model: model) }
                Button {
                    withAnimation(.snappy(duration: 0.2)) { showRanges.toggle() }
                } label: {
                    HStack {
                        Text("Ranges").font(.callout)
                        Spacer()
                        Image(systemName: "chevron.down").rotationEffect(.degrees(showRanges ? 180 : 0))
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                if showRanges {
                    ForEach(Array(chosen.ranges.enumerated()), id: \.offset) { which, range in
                        RangeRail(range: range) { t in
                            model.live?.nearestRangePoint(swatch: chosen.index, which: UInt32(which), t: t) ?? 0
                        } move: { point, t, done in
                            model.live?.setPointColorRange(swatch: chosen.index, which: UInt32(which), point: point, t: t, done: done)
                            model.refresh()
                        }
                    }
                }
                SwiftUI.Toggle("Visualize Range", isOn: $visualize).font(.callout)
            }
        }
        // Visualize Range follows the chosen swatch, and ends with the tab.
        .onChange(of: visualize ? chosen?.index : nil, initial: true) { _, shown in
            model.live?.setVisualizePointColor(index: shown)
        }
        .onDisappear { model.live?.setVisualizePointColor(index: nil) }
    }

    private func swatch(_ s: SwatchInfo, chosen: Bool) -> some View {
        Button { model.pointColorSelected = s.index } label: {
            Self.color(s.color)
                .frame(width: 22, height: 22)
                .clipShape(Circle())
                .overlay(Circle().strokeBorder(.white, lineWidth: chosen ? 2 : 0).padding(-3))
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button("Delete Swatch", systemImage: "trash", role: .destructive) {
                model.live?.deletePointColor(index: s.index)
                model.pointColorSelected = nil
                model.refresh()
            }
            Button("Delete All Swatches", role: .destructive) {
                model.live?.deletePointColor(index: nil)
                model.pointColorSelected = nil
                model.refresh()
            }
        }
        .accessibilityLabel("Swatch \(s.index + 1)")
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }

    private static func color(_ rgb: [Float]) -> Color {
        Color(red: Double(rgb[safe: 0] ?? 0), green: Double(rgb[safe: 1] ?? 0), blue: Double(rgb[safe: 2] ?? 0))
    }
}

/// One of a swatch's ranges: a rail in its colours with the outer and inner
/// points as handles. Dragging moves the nearest handle; a double tap resets
/// the range.
struct RangeRail: View {
    let range: RangeInfo
    /// The handle a press at a point along the rail (0–1) takes.
    let nearest: (Float) -> UInt32
    /// Moves handle `point` (or resets the range, `nil`) to a point along the
    /// rail; the flag ends the drag.
    let move: (UInt32?, Float, Bool) -> Void
    @State private var handle: UInt32?
    @State private var lastTap: Date?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(range.label).font(.caption).foregroundStyle(Theme.secondaryText)
            GeometryReader { geometry in
                let w = geometry.size.width
                let x = { (t: Float) in CGFloat(t) * w }
                let p = range.points.count == 4 ? range.points : [0, 0.33, 0.67, 1]
                ZStack(alignment: .topLeading) {
                    Capsule().fill(LinearGradient(colors: colors, startPoint: .leading, endPoint: .trailing))
                        .frame(height: 6).offset(y: 9)
                    Path { path in
                        path.move(to: CGPoint(x: x(p[0]), y: 18))
                        path.addLine(to: CGPoint(x: x(p[1]), y: 4))
                        path.addLine(to: CGPoint(x: x(p[2]), y: 4))
                        path.addLine(to: CGPoint(x: x(p[3]), y: 18))
                    }
                    .stroke(.white.opacity(0.8), lineWidth: 1)
                    ForEach(0..<4, id: \.self) { k in
                        Triangle()
                            .fill(k == 1 || k == 2 ? Color.white : Theme.panel)
                            .overlay(Triangle().stroke(.white, lineWidth: 1))
                            .frame(width: 10, height: 8)
                            .position(x: x(p[k]), y: 24)
                    }
                }
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        let t = Float(min(max(drag.location.x / max(w, 1), 0), 1))
                        if handle == nil { handle = nearest(t) }
                        if hypot(drag.translation.width, drag.translation.height) > 3 { move(handle, t, false) }
                    }
                    .onEnded { drag in
                        let t = Float(min(max(drag.location.x / max(w, 1), 0), 1))
                        let tapped = hypot(drag.translation.width, drag.translation.height) <= 3
                        if tapped, let last = lastTap, Date().timeIntervalSince(last) < 0.45 {
                            move(nil, 0, true)
                            lastTap = nil
                        } else if tapped {
                            lastTap = Date()
                        } else {
                            move(handle, t, true)
                            lastTap = nil
                        }
                        handle = nil
                    })
            }
            .frame(height: 30)
        }
        .accessibilityElement()
        .accessibilityLabel(range.label)
        .accessibilityValue(range.points.map { "\(Int(($0 * 100).rounded()))" }.joined(separator: ", "))
    }

    private var colors: [Color] {
        stride(from: 0, to: range.rail.count - 2, by: 3).map {
            Color(red: Double(range.rail[$0]), green: Double(range.rail[$0 + 1]), blue: Double(range.rail[$0 + 2]))
        }
    }
}

/// A triangle pointing up, for a range's handles.
struct Triangle: Shape {
    func path(in rect: CGRect) -> Path {
        Path { p in
            p.move(to: CGPoint(x: rect.midX, y: rect.minY))
            p.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
            p.addLine(to: CGPoint(x: rect.minX, y: rect.maxY))
            p.closeSubpath()
        }
    }
}

private extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
