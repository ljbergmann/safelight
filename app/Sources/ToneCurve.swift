import SwiftUI

/// The Tone Curve, as the desktop's: the parametric graph with its region
/// sliders, or the point curve with the Point Curve menu and Refine
/// Saturation. Levels follow below it, in their own section.
struct ToneCurveView: View {
    let model: LiveModel
    /// The "Tone Curve" section: its switch, the region sliders and Curve
    /// Saturation.
    let section: Section
    @State private var parametric = true
    @State private var saving = false
    @State private var saveName = ""
    @State private var note: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(section.switches, id: \.label) { SwitchRow(item: $0, model: model) }
            Picker("Curve", selection: $parametric) {
                Text("Parametric").tag(true)
                Text("Point").tag(false)
            }
            .pickerStyle(.segmented)
            if parametric {
                ParametricGraph(model: model)
                // Lightroom lists the regions lightest first, as the core does.
                ForEach(section.sliders.filter { $0.label != "Saturation" && !$0.label.contains("Saturation") }, id: \.label) {
                    SliderRow(slider: $0, model: model)
                }
            } else {
                PointCurveEditor(model: model)
                pointCurveMenu
                Text("Refine").font(.subheadline.weight(.semibold)).padding(.top, 4)
                ForEach(section.sliders.filter { $0.label.contains("Saturation") }, id: \.label) {
                    SliderRow(slider: $0, model: model)
                }
            }
            if let note { Text(note).font(.caption).foregroundStyle(Theme.secondaryText) }
        }
        .alert("Save Point Curve", isPresented: $saving) {
            TextField("Name", text: $saveName)
            Button("Save") {
                do {
                    try model.live?.savePointCurve(name: saveName)
                    note = "Saved \(saveName)"
                } catch {
                    note = "Not saved: \(error)"
                }
                model.refresh()
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Saves the RGB, Red, Green and Blue curves.")
        }
    }

    /// Lightroom's Point Curve menu: its curves, saved ones, and Save….
    private var pointCurveMenu: some View {
        let menu = model.edit == nil ? nil : model.live?.pointCurveMenu()
        return HStack {
            Text("Point Curve").foregroundStyle(Theme.secondaryText)
            Spacer()
            Menu {
                ForEach(Array((menu?.names ?? []).enumerated()), id: \.offset) { i, name in
                    if i == Int(menu?.builtin ?? 0), i > 0 { Divider() }
                    Button {
                        model.live?.choosePointCurve(index: UInt32(i))
                        model.refresh()
                    } label: {
                        if name == menu?.shown { Label(name, systemImage: "checkmark") } else { Text(name) }
                    }
                }
                Divider()
                Button("Save…", systemImage: "square.and.arrow.down") { saveName = ""; saving = true }
            } label: {
                HStack(spacing: 4) {
                    Text(menu?.shown ?? "—")
                    Image(systemName: "chevron.down").font(.caption)
                }
            }
        }
    }
}

/// The parametric curve: a drag up or down moves the region under the
/// finger, a double tap resets it, and the three handles below move the
/// splits between the regions (a double tap puts one back).
struct ParametricGraph: View {
    let model: LiveModel
    /// The slider a drag is moving and its value when it started.
    @State private var drag: (SliderState, Float)?
    @State private var split: UInt32?
    @State private var lastTap: Date?

    var body: some View {
        let _ = model.dragging?.1
        let curve = model.edit == nil ? nil : model.live?.parametricCurve()
        VStack(spacing: 4) {
            GeometryReader { geometry in
                let w = geometry.size.width, h = geometry.size.height
                ZStack {
                    RoundedRectangle(cornerRadius: 6).fill(Color.black.opacity(0.3))
                    Path { p in
                        for i in 1...3 {
                            let x = w * CGFloat(i) / 4, y = h * CGFloat(i) / 4
                            p.move(to: CGPoint(x: x, y: 0)); p.addLine(to: CGPoint(x: x, y: h))
                            p.move(to: CGPoint(x: 0, y: y)); p.addLine(to: CGPoint(x: w, y: y))
                        }
                    }
                    .stroke(Theme.line, lineWidth: 0.5)
                    if let samples = curve?.samples, samples.count > 1 {
                        Path { p in
                            for (i, v) in samples.enumerated() {
                                let point = CGPoint(x: w * CGFloat(i) / CGFloat(samples.count - 1), y: h * (1 - CGFloat(v)))
                                if i == 0 { p.move(to: point) } else { p.addLine(to: point) }
                            }
                        }
                        .stroke(.white, lineWidth: 2)
                    }
                }
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .onChanged { g in
                        if drag == nil,
                           let control = model.live?.parametricRegionControl(x: Float(g.startLocation.x / max(w, 1))),
                           let slider = model.edit?.sections.flatMap(\.sliders).first(where: { $0.control == control }) {
                            drag = (slider, slider.value)
                        }
                        guard let (slider, from) = drag else { return }
                        let value = min(max(from - Float(g.translation.height / h * 2), slider.min), slider.max)
                        model.set(slider.control, to: value, done: false)
                    }
                    .onEnded { g in
                        guard let (slider, from) = drag else { return }
                        drag = nil
                        if abs(g.translation.height) < 4 {
                            // A double tap puts the region back.
                            if let last = lastTap, Date().timeIntervalSince(last) < 0.45 {
                                model.set(slider.control, to: 0, done: true)
                                lastTap = nil
                            } else {
                                model.set(slider.control, to: from, done: true)
                                lastTap = Date()
                            }
                            return
                        }
                        let value = min(max(from - Float(g.translation.height / h * 2), slider.min), slider.max)
                        model.set(slider.control, to: value, done: true)
                    })
            }
            .aspectRatio(1, contentMode: .fit)
            splitHandles(curve?.splits ?? [0.25, 0.5, 0.75])
        }
    }

    private func splitHandles(_ splits: [Float]) -> some View {
        GeometryReader { geometry in
            let w = geometry.size.width
            ZStack(alignment: .topLeading) {
                Capsule().fill(Theme.line).frame(height: 2).offset(y: 3)
                ForEach(0..<min(splits.count, 3), id: \.self) { i in
                    Triangle().fill(Color.white)
                        .frame(width: 12, height: 10)
                        .position(x: CGFloat(splits[i]) * w, y: 12)
                        .gesture(DragGesture(minimumDistance: 0)
                            .onChanged { g in
                                model.live?.setParametricSplit(index: UInt32(i), t: Float(g.location.x / max(w, 1)), done: false)
                                model.refresh()
                            }
                            .onEnded { g in
                                if abs(g.translation.width) < 3 {
                                    model.live?.setParametricSplit(index: UInt32(i), t: nil, done: true)
                                } else {
                                    model.live?.setParametricSplit(index: UInt32(i), t: Float(g.location.x / max(w, 1)), done: true)
                                }
                                model.refresh()
                            })
                }
            }
        }
        .frame(height: 22)
        .accessibilityLabel("Region splits")
    }
}
