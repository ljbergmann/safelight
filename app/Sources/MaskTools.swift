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

/// The Masks and Heal panels: new masks, the list, the selected mask's
/// sliders, and the retouch tool.
struct MasksPanel: View {
    @Bindable var model: LiveModel
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Button("Brush", systemImage: "paintbrush.pointed") { model.addMask(.brush, from: [0, 0], to: [0, 0]) }
                    Button("Linear", systemImage: "rectangle.lefthalf.inset.filled") { model.tool = .newGradient(.linear) }
                    Button("Radial", systemImage: "circle.dashed") { model.tool = .newGradient(.radial) }
                    Menu {
                        Button("Color Range", systemImage: "eyedropper") { model.tool = .newGradient(.colorRange) }
                        Button("Luminance Range", systemImage: "sun.max") { model.tool = .newGradient(.luminanceRange) }
                    } label: {
                        Label("Range", systemImage: "slider.horizontal.below.rectangle")
                    }
                }
                .buttonStyle(.bordered).font(.callout)
                SelectButtons(model: model)
                switch model.tool {
                case .newGradient(.colorRange), .newGradient(.luminanceRange),
                     .newPart(.colorRange, _), .newPart(.luminanceRange, _):
                    Text("Tap the colour or brightness to select.").font(.caption).foregroundStyle(.secondary)
                case .newGradient, .newPart:
                    Text("Drag on the photo to place it.").font(.caption).foregroundStyle(.secondary)
                default:
                    EmptyView()
                }
                ForEach(model.edit?.masks ?? [], id: \.index) { mask in
                    HStack {
                        Button {
                            model.selectedMask = model.selectedMask == mask.index ? nil : mask.index
                            model.tool = (model.selectedMask != nil && mask.components.contains { $0.kind == "Brush" }) ? .brush : .none
                        } label: {
                            Label("\(mask.name) · \(mask.kind)", systemImage: model.selectedMask == mask.index ? "largecircle.fill.circle" : "circle")
                        }
                        Spacer()
                        Button { model.live?.setMaskHidden(index: mask.index, hidden: !mask.hidden); model.refresh() } label: {
                            Image(systemName: mask.hidden ? "eye.slash" : "eye")
                        }
                        Button(role: .destructive) { model.deleteMask(mask.index) } label: { Image(systemName: "trash") }
                    }
                    .font(.callout)
                }
                if let index = model.selectedMask, let mask = model.edit?.masks.first(where: { $0.index == index }) {
                    MaskParts(model: model, mask: mask)
                    if model.tool == .brush {
                        BrushSettings(model: model)
                    }
                    ForEach(model.live?.maskSliders(index: index) ?? [], id: \.label) { slider in
                        SliderRow(slider: slider, model: model)
                    }
                }
            }
            .padding(.top, 4)
        } label: {
            Text("Masks").font(.headline)
        }

        DisclosureGroup("Heal") {
            VStack(alignment: .leading, spacing: 6) {
                SwiftUI.Toggle("Heal tool (tap the photo)", isOn: Binding(
                    get: { model.tool == .heal }, set: { model.tool = $0 ? .heal : .none }
                ))
                Picker("Mode", selection: $model.cloneSpots) {
                    Text("Heal").tag(false)
                    Text("Clone").tag(true)
                }
                .pickerStyle(.segmented)
                LabeledContent("Size") { Slider(value: $model.spotSize, in: 0.005...0.1) }
                Text("\(model.edit?.spots ?? 0) spots").font(.caption).foregroundStyle(.secondary)
            }
            .font(.callout)
            .padding(.top, 4)
        }
        .font(.headline)
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
