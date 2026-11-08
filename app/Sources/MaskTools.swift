import SwiftUI
import UIKit

/// What a touch on the photo does while a mask or retouch tool is open.
enum PhotoTool: Equatable {
    case none
    /// Draw a new gradient from the touch's start to its end; for a range
    /// mask, tap the colour or brightness to select.
    case newGradient(NewMask)
    /// The same, as a part of the selected mask, combined by the operation.
    case newPart(NewMask, MaskOperation)
    /// Paint into the selected brush mask.
    case brush
    /// Tap to place a Heal (or Clone) spot.
    case heal
    /// Tap something neutral to set white balance from it.
    case whiteBalance
}

extension LiveModel {
    /// The photo as rendered (cropped), fitted in a view of `size` points.
    func photoRect(in size: CGSize) -> CGRect {
        guard let s = live?.stats(), s.width > 0, s.height > 0 else { return CGRect(origin: .zero, size: size) }
        let w = CGFloat(s.width), h = CGFloat(s.height)
        let k = min(size.width / w, size.height / h)
        return CGRect(x: (size.width - w * k) / 2, y: (size.height - h * k) / 2, width: w * k, height: h * k)
    }

    /// A point of the view as fractions across the photo as shown.
    func viewPoint(_ p: CGPoint, in rect: CGRect) -> [Float] {
        [Float((p.x - rect.minX) / rect.width), Float((p.y - rect.minY) / rect.height)]
    }

    func addMask(_ kind: NewMask, from: [Float], to: [Float]) {
        if let index = live?.addMask(kind: kind, from: from, to: to) {
            selectedMask = index
            tool = kind == .brush ? .brush : .none
        }
        refresh()
    }

    /// Adds a part to the selected mask: a brush part at once (then paint),
    /// the others by dragging or tapping on the photo.
    func addPart(_ kind: NewMask, _ operation: MaskOperation) {
        guard let mask = selectedMask else { return }
        if kind == .brush {
            _ = live?.addMaskComponent(mask: mask, kind: .brush, operation: operation, from: [0, 0], to: [0, 0])
            tool = .brush
            refresh()
        } else {
            tool = .newPart(kind, operation)
        }
    }

    func deleteMask(_ index: UInt32) {
        live?.deleteMask(index: index)
        selectedMask = nil
        tool = .none
        refresh()
    }
}

/// The colours a mask can be shown in over the photo.
enum MaskColor: String, CaseIterable {
    case red, green, blue, yellow, white

    var rgb: (Int, Int, Int) {
        switch self {
        case .red: (230, 40, 40)
        case .green: (40, 210, 80)
        case .blue: (50, 120, 255)
        case .yellow: (250, 210, 30)
        case .white: (255, 255, 255)
        }
    }

    var color: Color {
        let (r, g, b) = rgb
        return Color(red: Double(r) / 255, green: Double(g) / 255, blue: Double(b) / 255)
    }
}

extension LiveModel {
    /// The colour mask `index` shows in over the photo: chosen per mask,
    /// red until then.
    func overlayColor(for index: UInt32) -> MaskColor { maskColors[index] ?? .red }

    /// Draws each mask's coverage small, white where it applies, for the
    /// list beside the photo; one at a time, off the main thread.
    func refreshMaskThumbs() async {
        guard let live else { return }
        var made: [UInt32: UIImage] = [:]
        for mask in edit?.masks ?? [] {
            let index = mask.index
            let overlay = await Task.detached(priority: .utility) { live.maskOverlay(index: index, maxEdge: 96) }.value
            if Task.isCancelled { return }
            if let overlay, let image = Self.coverage(overlay) { made[index] = image }
        }
        maskThumbs = made
    }

    /// A mask's weights as a grey image: white where it applies fully.
    nonisolated static func coverage(_ o: MaskOverlay) -> UIImage? {
        let (w, h) = (Int(o.width), Int(o.height))
        guard w > 0, h > 0, let provider = CGDataProvider(data: Data(o.alpha) as CFData),
              let cg = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 8, bytesPerRow: w,
                               space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGBitmapInfo(rawValue: 0),
                               provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
}

/// The Masks panel: how to make a mask while none is chosen; the chosen
/// mask's parts, brush and sliders. The masks themselves are listed over
/// the photo ([`MaskStrip`]).
struct MasksPanel: View {
    @Bindable var model: LiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Masks").font(.title3.weight(.semibold))
            switch model.tool {
            case .newGradient(.colorRange), .newGradient(.luminanceRange),
                 .newPart(.colorRange, _), .newPart(.luminanceRange, _):
                hint("Tap the colour or brightness to select.")
            case .newGradient, .newPart:
                hint("Drag on the photo to place it.")
            default:
                EmptyView()
            }
            if let what = model.selecting {
                HStack(spacing: 6) {
                    ProgressView().controlSize(.small)
                    Text("Selecting \(String(describing: what))…")
                }
                .font(.caption).foregroundStyle(Theme.secondaryText)
            } else if let error = model.selectionError {
                Text(error).font(.caption).foregroundStyle(.orange)
            }
            if let index = model.selectedMask, let mask = model.edit?.masks.first(where: { $0.index == index }) {
                HStack {
                    Text("\(mask.name) · \(mask.kind)").font(.headline)
                    Spacer()
                    Button { model.live?.setMaskHidden(index: mask.index, hidden: !mask.hidden); model.refresh() } label: {
                        Image(systemName: mask.hidden ? "eye.slash" : "eye")
                    }
                    Button(role: .destructive) { model.deleteMask(mask.index) } label: { Image(systemName: "trash") }
                }
                HStack(spacing: 10) {
                    Text("Overlay").foregroundStyle(Theme.secondaryText)
                    Spacer()
                    ForEach(MaskColor.allCases, id: \.self) { color in
                        Button {
                            model.maskColors[index] = color
                            model.showMaskOverlay = true
                        } label: {
                            Circle().fill(color.color).frame(width: 22, height: 22)
                                .overlay(Circle().stroke(.white, lineWidth: model.overlayColor(for: index) == color && model.showMaskOverlay ? 2 : 0).padding(-3))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Show in \(color.rawValue)")
                    }
                }
                MaskParts(model: model, mask: mask)
                if model.tool == .brush {
                    BrushSettings(model: model)
                }
                MaskSettings(model: model, mask: index)
            } else {
                hint(model.edit?.masks.isEmpty ?? true
                     ? "Add a mask with + beside the photo: a brush, a gradient, a range, or the subject, sky or background."
                     : "Choose a mask beside the photo to change it, or add one with +.")
            }
        }
    }

    private func hint(_ text: String) -> some View {
        Text(text).font(.callout).foregroundStyle(Theme.secondaryText)
    }
}

/// The photo's masks, in a column over its right edge as Lightroom shows
/// them: + to add one, then each mask as a tile; tapping one chooses it.
struct MaskStrip: View {
    @Bindable var model: LiveModel

    var body: some View {
        VStack(spacing: 8) {
            Menu {
                SwiftUI.Section("Select") {
                    Button("Subject", systemImage: "person.crop.rectangle") { model.select(.subject) }
                    Button("Sky", systemImage: "cloud.sun") { model.select(.sky) }
                    Button("Background", systemImage: "mountain.2") { model.select(.background) }
                }
                SwiftUI.Section("Draw") {
                    Button("Brush", systemImage: "paintbrush.pointed") { model.addMask(.brush, from: [0, 0], to: [0, 0]) }
                    Button("Linear Gradient", systemImage: "rectangle.lefthalf.inset.filled") { model.tool = .newGradient(.linear) }
                    Button("Radial Gradient", systemImage: "circle.dashed") { model.tool = .newGradient(.radial) }
                }
                SwiftUI.Section("Range") {
                    Button("Color Range", systemImage: "eyedropper") { model.tool = .newGradient(.colorRange) }
                    Button("Luminance Range", systemImage: "sun.max") { model.tool = .newGradient(.luminanceRange) }
                }
            } label: {
                Image(systemName: "plus").font(.title3.weight(.semibold)).foregroundStyle(.white)
                    .frame(width: 44, height: 44)
                    .background(Theme.accent, in: Circle())
            }
            // As written, whichever way it opens.
            .menuOrder(.fixed)
            .disabled(model.selecting != nil)
            ForEach(model.edit?.masks ?? [], id: \.index) { mask in
                Button {
                    model.selectedMask = model.selectedMask == mask.index ? nil : mask.index
                    model.tool = (model.selectedMask != nil && mask.components.contains { $0.kind == "Brush" }) ? .brush : .none
                } label: {
                    Group {
                        if let thumb = model.maskThumbs[mask.index] {
                            Image(uiImage: thumb).resizable().scaledToFill()
                        } else {
                            Image(systemName: Self.symbol(mask.kind)).font(.title3)
                        }
                    }
                    .frame(width: 48, height: 48)
                    .background(Theme.chosen)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                        .overlay(RoundedRectangle(cornerRadius: 8)
                            .stroke(model.selectedMask == mask.index ? Theme.accent : .clear, lineWidth: 2))
                        .opacity(mask.hidden ? 0.4 : 1)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("\(mask.name), \(mask.kind)")
            }
        }
        .padding(8)
        .background(Theme.raised.opacity(0.92), in: RoundedRectangle(cornerRadius: 12))
        // Drawn again when the masks or their shapes change.
        .task(id: "\(model.edit?.masks.map(\.name) ?? [])-\(model.edit?.applied ?? 0)-\(model.edit?.steps.count ?? 0)") {
            try? await Task.sleep(for: .milliseconds(300))
            if Task.isCancelled { return }
            await model.refreshMaskThumbs()
        }
    }

    static func symbol(_ kind: String) -> String {
        switch kind {
        case "Brush": "paintbrush.pointed"
        case "Linear Gradient": "rectangle.lefthalf.inset.filled"
        case "Radial Gradient": "circle.dashed"
        case "Color Range": "eyedropper"
        case "Luminance Range": "sun.max"
        case "Subject": "person.crop.rectangle"
        case "Sky": "cloud.sun"
        case "Background": "mountain.2"
        default: "circle.dashed.inset.filled"
        }
    }
}

/// A mask's sliders as Lightroom's Masking panel groups them: Amount and a
/// range's sliders first, then Light, Color, Effects and Detail, folding.
struct MaskSettings: View {
    let model: LiveModel
    let mask: UInt32
    @State private var open: Set<String> = ProcessInfo.processInfo.arguments.contains("-show-mask") ? ["Color"] : ["Light"]

    var body: some View {
        // Read through `model.edit`, which every change refreshes, so the
        // sliders show the values just set: `live` itself never changes.
        let sections = model.edit == nil ? [] : (model.live?.maskSections(index: mask) ?? [])
        VStack(alignment: .leading, spacing: 0) {
            ForEach(sections.filter { $0.group == "Mask" }, id: \.title) { section in
                ForEach(section.sliders, id: \.label) { SliderRow(slider: $0, model: model) }
            }
            ForEach(sections.filter { $0.group != "Mask" }, id: \.title) { section in
                Rectangle().fill(Theme.line).frame(height: 0.5).padding(.top, 8)
                Button {
                    withAnimation(.snappy(duration: 0.2)) {
                        if open.contains(section.title) { open.remove(section.title) } else { open.insert(section.title) }
                    }
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: "chevron.right").font(.footnote.weight(.semibold))
                            .rotationEffect(.degrees(open.contains(section.title) ? 90 : 0))
                        Text(section.title).font(.headline)
                        Spacer()
                    }
                    .padding(.vertical, 10).contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                if open.contains(section.title) {
                    ForEach(section.sliders, id: \.label) { slider in
                        SliderRow(slider: slider, model: model, track: SliderRow.track(for: slider.label, in: section.title))
                    }
                }
            }
        }
    }
}

/// Heal and Clone: tap the photo to place a spot, which takes its source
/// from nearby.
struct HealPane: View {
    @Bindable var model: LiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Heal").font(.title3.weight(.semibold))
            Picker("Mode", selection: $model.cloneSpots) {
                Text("Heal").tag(false)
                Text("Clone").tag(true)
            }
            .pickerStyle(.segmented)
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Text("Size").foregroundStyle(Theme.secondaryText)
                    Spacer()
                    Text("\(Int((model.spotSize * 1000).rounded()))").monospacedDigit()
                }
                ThinSlider(value: model.spotSize, range: 0.005...0.1) { value, _ in model.spotSize = value }
            }
            Text("Tap a spot on the photo to \(model.cloneSpots ? "clone over" : "heal") it.")
                .font(.callout).foregroundStyle(Theme.secondaryText)
            Text("\(model.edit?.spots ?? 0) spot\(model.edit?.spots == 1 ? "" : "s") on this photo")
                .font(.caption).foregroundStyle(Theme.secondaryText)
        }
        .onAppear { model.tool = .heal }
        .onDisappear { if model.tool == .heal { model.tool = .none } }
    }
}

struct BrushSettings: View {
    @Bindable var model: LiveModel
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            LabeledContent("Size") { Slider(value: $model.brushSize, in: 0.005...0.2) }
            LabeledContent("Feather") { Slider(value: $model.brushFeather, in: 0...1) }
            LabeledContent("Flow") { Slider(value: $model.brushFlow, in: 0.05...1) }
            SwiftUI.Toggle("Erase", isOn: $model.brushErase)
            Text("Paint with a finger or the Pencil; the Pencil's pressure sets the flow.")
                .font(.caption).foregroundStyle(.secondary)
        }
        .font(.callout)
    }
}

/// Draws over the photo while a mask or retouch tool is open: gradients,
/// brush strokes (with Pencil pressure) and Heal spots.
struct PhotoToolOverlay: UIViewRepresentable {
    let model: LiveModel
    let size: CGSize

    func makeUIView(context: Context) -> PaintView {
        let view = PaintView()
        view.backgroundColor = .clear
        view.isMultipleTouchEnabled = false
        return view
    }

    func updateUIView(_ view: PaintView, context: Context) {
        view.model = model
        view.photo = model.photoRect(in: size)
    }
}

final class PaintView: UIView {
    var model: LiveModel?
    var photo: CGRect = .zero
    private var points: [CGPoint] = []
    private var pressures: [CGFloat] = []
    private let trail = CAShapeLayer()

    override init(frame: CGRect) {
        super.init(frame: frame)
        trail.strokeColor = UIColor.systemRed.withAlphaComponent(0.6).cgColor
        trail.fillColor = UIColor.clear.cgColor
        trail.lineCap = .round
        trail.lineJoin = .round
        layer.addSublayer(trail)
    }
    required init?(coder: NSCoder) { fatalError() }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let t = touches.first else { return }
        points = [t.location(in: self)]
        pressures = [pressure(t)]
        draw()
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let t = touches.first else { return }
        for c in event?.coalescedTouches(for: t) ?? [t] {
            points.append(c.location(in: self))
            pressures.append(pressure(c))
        }
        draw()
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        MainActor.assumeIsolated { finish() }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        points = []; draw()
    }

    /// A Pencil's force as 0–1; a finger paints at the brush's own flow.
    private func pressure(_ t: UITouch) -> CGFloat {
        t.type == .pencil && t.maximumPossibleForce > 0 ? t.force / t.maximumPossibleForce : -1
    }

    private func draw() {
        MainActor.assumeIsolated {
            guard let model, model.tool == .brush, photo.width > 0 else { trail.path = nil; return }
            let path = UIBezierPath()
            if let first = points.first { path.move(to: first) }
            points.dropFirst().forEach { path.addLine(to: $0) }
            trail.lineWidth = CGFloat(model.brushSize) * max(photo.width, photo.height) * 2
            trail.path = path.cgPath
        }
    }

    @MainActor private func finish() {
        defer { points = []; trail.path = nil }
        guard let model, let first = points.first, let last = points.last, photo.width > 0 else { return }
        let view = { (p: CGPoint) in model.viewPoint(p, in: self.photo) }
        switch model.tool {
        case .newGradient(let kind):
            // A range mask samples where the touch ended.
            let from = kind == .colorRange || kind == .luminanceRange ? view(last) : view(first)
            model.addMask(kind, from: from, to: view(last))
        case .newPart(let kind, let op):
            guard let mask = model.selectedMask else { return }
            let from = kind == .colorRange || kind == .luminanceRange ? view(last) : view(first)
            _ = model.live?.addMaskComponent(mask: mask, kind: kind, operation: op, from: from, to: view(last))
            model.tool = .none
            model.refresh()
        case .brush:
            guard let index = model.selectedMask else { return }
            let pencil = pressures.filter { $0 >= 0 }
            let flow = pencil.isEmpty ? model.brushFlow : Float(pencil.reduce(0, +) / CGFloat(pencil.count))
            model.live?.brush(index: index, stroke: Stroke(
                points: points.flatMap(view), radius: model.brushSize, feather: model.brushFeather,
                flow: max(0.05, flow), erase: model.brushErase))
            model.refresh()
        case .heal:
            _ = model.live?.addSpot(at: view(last), radius: model.spotSize, clone: model.cloneSpots)
            model.refresh()
        case .whiteBalance:
            model.live?.pickWhiteBalance(at: view(last))
            model.tool = .none
            model.refresh()
        case .none:
            break
        }
    }
}

/// The selected mask's parts: each with how it combines and Invert, Add,
/// Subtract and Intersect to add one, and the whole mask's Invert.
struct MaskParts: View {
    let model: LiveModel
    let mask: MaskInfo

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            SwiftUI.Toggle("Show overlay", isOn: Bindable(model).showMaskOverlay)
            SwiftUI.Toggle("Invert mask", isOn: Binding(
                get: { mask.invert },
                set: { model.live?.setMaskInvert(mask: mask.index, component: nil, invert: $0); model.refresh() }
            ))
            ForEach(mask.components, id: \.index) { part in
                HStack {
                    Text(part.index == 0 ? part.kind : "\(name(part.operation)) \(part.kind)")
                    Spacer()
                    Button {
                        model.live?.setMaskInvert(mask: mask.index, component: part.index, invert: !part.invert)
                        model.refresh()
                    } label: {
                        Image(systemName: part.invert ? "circle.lefthalf.filled.inverse" : "circle.lefthalf.filled")
                    }
                    if mask.components.count > 1 {
                        Button(role: .destructive) {
                            model.live?.deleteMaskComponent(mask: mask.index, component: part.index)
                            model.refresh()
                        } label: { Image(systemName: "minus.circle") }
                    }
                }
                .font(.caption)
            }
            HStack {
                ForEach([MaskOperation.add, .subtract, .intersect], id: \.self) { op in
                    Menu(name(op)) {
                        Button("Brush") { model.addPart(.brush, op) }
                        Button("Linear Gradient") { model.addPart(.linear, op) }
                        Button("Radial Gradient") { model.addPart(.radial, op) }
                        Button("Color Range") { model.addPart(.colorRange, op) }
                        Button("Luminance Range") { model.addPart(.luminanceRange, op) }
                    }
                }
            }
            .buttonStyle(.bordered).font(.caption)
        }
        .font(.callout)
    }

    private func name(_ op: MaskOperation) -> String {
        switch op {
        case .add: "Add"
        case .subtract: "Subtract"
        case .intersect: "Intersect"
        }
    }
}

/// Over the photo while a mask is selected (at Fit): its coverage in red, and
/// a placed gradient's handles, dragged to move or reshape it.
struct MaskOnPhoto: View {
    let model: LiveModel
    let size: CGSize
    @State private var overlay: UIImage?
    @State private var last: [Float]?

    var body: some View {
        let rect = model.photoRect(in: size)
        ZStack(alignment: .topLeading) {
            if model.showMaskOverlay, let overlay {
                Image(uiImage: overlay)
                    .resizable()
                    .frame(width: rect.width, height: rect.height)
                    .offset(x: rect.minX, y: rect.minY)
                    .allowsHitTesting(false)
            }
            if let mask = model.selectedMask {
                ForEach(gradientParts(mask), id: \.self) { part in
                    let points = model.live?.gradientHandles(mask: mask, component: part) ?? []
                    ForEach(0..<points.count / 2, id: \.self) { h in
                        let p = CGPoint(x: rect.minX + CGFloat(points[2 * h]) * rect.width,
                                        y: rect.minY + CGFloat(points[2 * h + 1]) * rect.height)
                        Circle()
                            .fill(.white)
                            .stroke(.black.opacity(0.5), lineWidth: 1)
                            .frame(width: h == 0 ? 16 : 12, height: h == 0 ? 16 : 12)
                            .padding(10)
                            .contentShape(Circle())
                            .position(p)
                            .gesture(
                                DragGesture(minimumDistance: 0, coordinateSpace: .local)
                                    .onChanged { drag(mask, part, h, to: $0.location, in: rect, done: false) }
                                    .onEnded { drag(mask, part, h, to: $0.location, in: rect, done: true) }
                            )
                    }
                }
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .task(id: "\(model.selectedMask ?? 99)-\(model.edit?.applied ?? 0)-\(model.edit?.steps.count ?? 0)-\(model.showMaskOverlay)-\(model.overlayColor(for: model.selectedMask ?? 0))") {
            await loadOverlay()
        }
    }

    /// The parts of `mask` that are gradients, whose handles show.
    private func gradientParts(_ mask: UInt32) -> [UInt32] {
        let parts = model.edit?.masks.first { $0.index == mask }?.components ?? []
        return parts.filter { $0.kind.hasSuffix("Gradient") }.map(\.index)
    }

    private func drag(_ mask: UInt32, _ part: UInt32, _ handle: Int, to location: CGPoint, in rect: CGRect, done: Bool) {
        let at = model.viewPoint(location, in: rect)
        let from = last ?? at
        model.live?.dragGradientHandle(mask: mask, component: part, handle: UInt32(handle), from: from, to: at, done: done)
        last = done ? nil : at
        if done { model.refresh() }
    }

    private func loadOverlay() async {
        guard model.showMaskOverlay, let live = model.live, let mask = model.selectedMask else {
            overlay = nil
            return
        }
        try? await Task.sleep(for: .milliseconds(150))
        if Task.isCancelled { return }
        let made = await Task.detached { live.maskOverlay(index: mask, maxEdge: 640) }.value
        guard let made, !Task.isCancelled else { return }
        overlay = Self.image(made, color: model.overlayColor(for: mask))
    }

    /// The coverage in the mask's overlay colour, whose opacity is the
    /// mask's at half strength, as the desktop shows it.
    private static func image(_ o: MaskOverlay, color: MaskColor) -> UIImage? {
        let (w, h) = (Int(o.width), Int(o.height))
        let (r, g, b) = color.rgb
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        for i in 0..<(w * h) {
            let a = Int(o.alpha[i]) / 2
            // Premultiplied.
            rgba[4 * i] = UInt8(a * r / 255)
            rgba[4 * i + 1] = UInt8(a * g / 255)
            rgba[4 * i + 2] = UInt8(a * b / 255)
            rgba[4 * i + 3] = UInt8(a)
        }
        guard let provider = CGDataProvider(data: Data(rgba) as CFData),
              let cg = CGImage(width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                               space: CGColorSpaceCreateDeviceRGB(),
                               bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                               provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return nil }
        return UIImage(cgImage: cg)
    }
}
