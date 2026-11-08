import SwiftUI

/// The Crop tool's aspect: free, the photo's own, or a ratio (long side over
/// short, turned to the crop's orientation).
enum CropAspect: Hashable {
    case free
    case original
    case ratio(Float)

    static let presets: [(String, CropAspect)] = [
        ("Free", .free), ("Original", .original), ("1 × 1", .ratio(1)), ("4 × 5", .ratio(5.0 / 4)),
        ("3 × 2", .ratio(3.0 / 2)), ("4 × 3", .ratio(4.0 / 3)), ("16 × 9", .ratio(16.0 / 9)),
    ]
}

/// The crop as fractions of the photo as shown, straightened: `[left, top,
/// right, bottom]`, the recipe's own coordinates.
struct CropRect: Equatable {
    var l: Float, t: Float, r: Float, b: Float

    init(_ v: [Float]) { (l, t, r, b) = v.count == 4 ? (v[0], v[1], v[2], v[3]) : (0, 0, 1, 1) }
    init(l: Float, t: Float, r: Float, b: Float) { (self.l, self.t, self.r, self.b) = (l, t, r, b) }
    var array: [Float] { [l, t, r, b] }
    var width: Float { r - l }
    var height: Float { b - t }
}

/// Where a crop drag started: which handle, or inside to move it.
extension CropOverlay {
    /// Keeps `frame` to the drawn frame's size while the tool is open: the
    /// photo may still be loading, or showing its cropped frame, when it
    /// opens.
    fileprivate func follow() async {
        while !Task.isCancelled {
            if let s = model.live?.stats() {
                let now = CGSize(width: CGFloat(s.frameWidth), height: CGFloat(s.frameHeight))
                if now != frame { frame = now }
            }
            try? await Task.sleep(for: .milliseconds(100))
        }
    }
}

private enum Handle {
    case corner(left: Bool, top: Bool)
    case side(Edge)
    case move
}

/// The crop drawn over the whole straightened photo, with handles to resize
/// it and room inside to move it.
struct CropOverlay: View {
    let model: LiveModel
    /// The view's size in points.
    let size: CGSize

    @State private var start: (Handle, CropRect)?
    @State private var shown: CropRect?
    /// The size of the frame on screen, in pixels: the whole straightened
    /// photo once the Crop tool's first frame is drawn. Followed by `follow`,
    /// as the core's frame stats aren't something SwiftUI watches.
    @State private var frame = CGSize.zero

    var body: some View {
        let photo = fitted(frame)
        let crop = shown ?? CropRect(model.edit?.crop ?? [0, 0, 1, 1])
        let rect = CGRect(
            x: photo.minX + CGFloat(crop.l) * photo.width, y: photo.minY + CGFloat(crop.t) * photo.height,
            width: CGFloat(crop.width) * photo.width, height: CGFloat(crop.height) * photo.height)
        ZStack {
            // Outside the crop, dimmed.
            Path { p in
                p.addRect(photo)
                p.addRect(rect)
            }
            .fill(Color.black.opacity(0.55), style: FillStyle(eoFill: true))
            // The guide overlay chosen in the Crop pane, as the desktop draws it.
            if let guide = model.cropGuide {
                Path { p in
                    for line in cropGuideLines(index: guide, orientation: model.cropGuideOrientation,
                                               width: Float(rect.width), height: Float(rect.height)) {
                        let points = stride(from: 0, to: line.count - 1, by: 2).map {
                            CGPoint(x: rect.minX + CGFloat(line[$0]), y: rect.minY + CGFloat(line[$0 + 1]))
                        }
                        p.addLines(points)
                    }
                }
                .stroke(Color.white.opacity(0.6), lineWidth: 0.75)
            }
            Rectangle().path(in: rect).stroke(Color.white, lineWidth: 1.5)
            ForEach(0..<4, id: \.self) { i in
                let corner = CGPoint(x: i % 2 == 0 ? rect.minX : rect.maxX, y: i < 2 ? rect.minY : rect.maxY)
                Path { p in
                    let dx: CGFloat = i % 2 == 0 ? 1 : -1, dy: CGFloat = i < 2 ? 1 : -1
                    p.move(to: CGPoint(x: corner.x, y: corner.y + 22 * dy))
                    p.addLine(to: corner)
                    p.addLine(to: CGPoint(x: corner.x + 22 * dx, y: corner.y))
                }
                .stroke(Color.white, lineWidth: 4)
            }
        }
        .contentShape(Rectangle())
        .opacity(frame.width > 0 ? 1 : 0)
        .task { await follow() }
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { value in drag(value, photo: photo, rect: rect, crop: crop) }
                .onEnded { _ in
                    if let shown { model.setCrop(shown, done: true) }
                    start = nil
                    shown = nil
                }
        )
    }

    /// The straightened photo fitted in the view, as the core draws it.
    private func fitted(_ frame: CGSize) -> CGRect {
        guard frame.width > 0, frame.height > 0 else { return CGRect(origin: .zero, size: size) }
        let s = min(size.width / frame.width, size.height / frame.height)
        let w = frame.width * s, h = frame.height * s
        return CGRect(x: (size.width - w) / 2, y: (size.height - h) / 2, width: w, height: h)
    }

    private func drag(_ value: DragGesture.Value, photo: CGRect, rect: CGRect, crop: CropRect) {
        if start == nil {
            start = (handle(at: value.startLocation, rect: rect), crop)
        }
        guard let (handle, from) = start, photo.width > 0 else { return }
        let dx = Float(value.translation.width / photo.width)
        let dy = Float(value.translation.height / photo.height)
        var c = from
        switch handle {
        case .move:
            let x = min(max(dx, -c.l), 1 - c.r), y = min(max(dy, -c.t), 1 - c.b)
            c = CropRect(l: c.l + x, t: c.t + y, r: c.r + x, b: c.b + y)
        case .corner(let left, let top):
            if left { c.l = c.l + dx } else { c.r = c.r + dx }
            if top { c.t = c.t + dy } else { c.b = c.b + dy }
            c = constrained(c, anchor: (left ? from.r : from.l, top ? from.b : from.t), left: left, top: top)
        case .side(let edge):
            switch edge {
            case .leading: c.l += dx
            case .trailing: c.r += dx
            case .top: c.t += dy
            case .bottom: c.b += dy
            }
            c = constrainedSide(c, edge: edge, from: from)
        }
        shown = c
        model.setCrop(c, done: false)
    }

    /// The handle nearest `point`: a corner within 30 points, a side within
    /// 25, else inside to move the crop.
    private func handle(at point: CGPoint, rect: CGRect) -> Handle {
        for left in [true, false] {
            for top in [true, false] {
                let corner = CGPoint(x: left ? rect.minX : rect.maxX, y: top ? rect.minY : rect.maxY)
                if hypot(point.x - corner.x, point.y - corner.y) < 30 { return .corner(left: left, top: top) }
            }
        }
        if abs(point.x - rect.minX) < 25 { return .side(.leading) }
        if abs(point.x - rect.maxX) < 25 { return .side(.trailing) }
        if abs(point.y - rect.minY) < 25 { return .side(.top) }
        if abs(point.y - rect.maxY) < 25 { return .side(.bottom) }
        return .move
    }

    /// A corner drag, kept on the photo, at least 2% each way, and at the
    /// locked ratio when there is one, from the opposite corner.
    private func constrained(_ c: CropRect, anchor: (Float, Float), left: Bool, top: Bool) -> CropRect {
        var w = max(abs(left ? anchor.0 - c.l : c.r - anchor.0), 0.02)
        var h = max(abs(top ? anchor.1 - c.t : c.b - anchor.1), 0.02)
        let roomX = left ? anchor.0 : 1 - anchor.0
        let roomY = top ? anchor.1 : 1 - anchor.1
        if let k = model.cropRatio {
            // Width over height in the crop's own units is k: follow the
            // larger of the two, then shrink to fit.
            if w / h > k { h = w / k } else { w = h * k }
            let fit = min(1, roomX / w, roomY / h)
            w *= fit; h *= fit
        } else {
            w = min(w, roomX); h = min(h, roomY)
        }
        return CropRect(
            l: left ? anchor.0 - w : anchor.0, t: top ? anchor.1 - h : anchor.1,
            r: left ? anchor.0 : anchor.0 + w, b: top ? anchor.1 : anchor.1 + h)
    }

    /// A side drag, kept on the photo; with a locked ratio the other
    /// dimension follows about its centre.
    private func constrainedSide(_ c: CropRect, edge: Edge, from: CropRect) -> CropRect {
        var c = CropRect(l: max(0, c.l), t: max(0, c.t), r: min(1, c.r), b: min(1, c.b))
        if c.width < 0.02 { if edge == .leading { c.l = c.r - 0.02 } else { c.r = c.l + 0.02 } }
        if c.height < 0.02 { if edge == .top { c.t = c.b - 0.02 } else { c.b = c.t + 0.02 } }
        guard let k = model.cropRatio else { return c }
        if edge == .leading || edge == .trailing {
            let cy = (from.t + from.b) / 2
            var h = c.width / k
            h = min(h, 2 * min(cy, 1 - cy))
            let w = h * k
            if edge == .leading { c.l = c.r - w } else { c.r = c.l + w }
            c.t = cy - h / 2; c.b = cy + h / 2
        } else {
            let cx = (from.l + from.r) / 2
            var w = c.height * k
            w = min(w, 2 * min(cx, 1 - cx))
            let h = w / k
            if edge == .top { c.t = c.b - h } else { c.b = c.t + h }
            c.l = cx - w / 2; c.r = cx + w / 2
        }
        return c
    }
}

/// The Crop tool's controls, shown below the photo while it is open.
struct CropBar: View {
    let model: LiveModel

    var body: some View {
        HStack(spacing: 14) {
            Menu {
                ForEach(CropAspect.presets, id: \.0) { name, aspect in
                    Button(name) { model.chooseAspect(aspect) }
                }
            } label: {
                Label(aspectName, systemImage: model.cropAspect == .free ? "lock.open" : "lock")
            }
            Button("Swap", systemImage: "rectangle.portrait.rotate") { model.swapCropOrientation() }
                .disabled(model.cropAspect == .free)
            Divider().frame(height: 22)
            Button("Rotate Left", systemImage: "rotate.left") { model.orient(.rotateLeft) }
            Button("Rotate Right", systemImage: "rotate.right") { model.orient(.rotateRight) }
            Button("Flip", systemImage: "arrow.left.and.right.righttriangle.left.righttriangle.right") {
                model.orient(.flipHorizontal)
            }
            Spacer()
            Button("Reset") { model.setCrop(CropRect(l: 0, t: 0, r: 1, b: 1), done: true) }
            Button("Done") { model.cropping = false }.buttonStyle(.borderedProminent)
        }
        .labelStyle(.iconOnly)
        .font(.title3)
    }

    private var aspectName: String {
        CropAspect.presets.first { $0.1 == model.cropAspect }?.0 ?? "Custom"
    }
}

extension LiveModel {
    /// The straightened photo's width over height, as the core last drew it.
    var frameAspect: Float? {
        guard let s = live?.stats(), s.frameWidth > 0, s.frameHeight > 0 else { return nil }
        return s.frameWidth / s.frameHeight
    }

    /// The ratio crop gestures keep, as the crop's width over its height in
    /// its own 0–1 units (the desktop's `crop_constraint`); nil when free.
    var cropRatio: Float? {
        guard let frame = frameAspect else { return nil }
        let crop = CropRect(edit?.crop ?? [0, 0, 1, 1])
        let landscape = crop.width * frame >= crop.height
        let shown: Float
        switch cropAspect {
        case .free: return nil
        case .original: shown = frame
        case .ratio(let r): shown = (landscape != cropPortrait) ? r : 1 / r
        }
        return shown / frame
    }

    func setCrop(_ crop: CropRect, done: Bool) {
        live?.setCrop(crop: crop.array, done: done)
        refresh()
    }

    func orient(_ o: Orientation) {
        live?.orient(orientation: o)
        refresh()
    }

    /// Picks an aspect: the crop becomes the largest at that ratio about its
    /// centre, as one step.
    func chooseAspect(_ aspect: CropAspect) {
        cropAspect = aspect
        cropPortrait = false
        applyRatio()
    }

    /// X: the crop turns between portrait and landscape at the same aspect.
    func swapCropOrientation() {
        cropPortrait.toggle()
        applyRatio()
    }

    private func applyRatio() {
        guard let k = cropRatio else { return }
        let c = CropRect(edit?.crop ?? [0, 0, 1, 1])
        let cx = (c.l + c.r) / 2, cy = (c.t + c.b) / 2
        // As large as fits at ratio k about the centre.
        var w = 2 * min(cx, 1 - cx), h = 2 * min(cy, 1 - cy)
        if w / h > k { w = h * k } else { h = w / k }
        setCrop(CropRect(l: cx - w / 2, t: cy - h / 2, r: cx + w / 2, b: cy + h / 2), done: true)
    }
}
