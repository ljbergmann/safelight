import SwiftUI

/// The live view and what it should show. A class, so the layer view's
/// callback and SwiftUI's change handlers see the same state, whichever of
/// the view and the photo comes first.
@MainActor @Observable
final class LiveModel {
    var live: LiveView? { didSet { open() } }
    var photo: URL? { didSet { if photo != oldValue { open() } } }
    var size: DecodeSize = .half { didSet { if size != oldValue { open() } } }
    var exposure: Float = 0 { didSet { live?.setExposure(stops: exposure) } }
    /// Fit, or 100% around a centre in fractions of the developed photo.
    var zoom: Zoom = .fit { didSet { live?.setZoom(zoom: zoom) } }
    /// A pinch in progress, shown by scaling the last frame until it ends.
    var pinch: CGFloat = 1
    var pinchAnchor: UnitPoint = .center
    /// The centre a drag started from.
    var dragStart: (Float, Float)?

    private func open() {
        guard let live, let photo else { return }
        exposure = 0
        zoom = .fit
        live.open(path: photo.path, size: size)
    }

    /// The point of the developed photo under `location` (in the view's
    /// points), as fractions of its size, for a view `size` points large.
    func photoPoint(at location: CGPoint, in size: CGSize, scale: CGFloat) -> (Float, Float)? {
        guard let stats = live?.stats(), stats.fullWidth > 0 else { return nil }
        let (fw, fh) = (CGFloat(stats.fullWidth), CGFloat(stats.fullHeight))
        let (w, h) = (size.width * scale, size.height * scale)
        let (x, y) = (location.x * scale, location.y * scale)
        switch zoom {
        case .fit:
            let s = min(w / fw, h / fh)
            let (left, top) = ((w - fw * s) / 2, (h - fh * s) / 2)
            return (Float(min(max((x - left) / (fw * s), 0), 1)), Float(min(max((y - top) / (fh * s), 0), 1)))
        case .actual(let cx, let cy):
            return (cx + Float((x - w / 2) / fw), cy + Float((y - h / 2) / fh))
        }
    }

    /// Fit ↔ 100% at `location`.
    func toggleZoom(at location: CGPoint, in size: CGSize, scale: CGFloat) {
        if case .fit = zoom, let (x, y) = photoPoint(at: location, in: size, scale: scale) {
            zoom = .actual(centerX: x, centerY: y)
        } else {
            zoom = .fit
        }
    }

    /// Ends a pinch: in from Fit to 100% around its anchor, or out to Fit.
    func endPinch(in size: CGSize, scale: CGFloat) {
        let anchor = CGPoint(x: pinchAnchor.x * size.width, y: pinchAnchor.y * size.height)
        switch zoom {
        case .fit where pinch > 1.25:
            if let (x, y) = photoPoint(at: anchor, in: size, scale: scale) {
                zoom = .actual(centerX: x, centerY: y)
            }
        case .actual where pinch < 0.8:
            zoom = .fit
        default:
            break
        }
        pinch = 1
    }

    /// Pans at 100% by `translation` points from where the drag started.
    func drag(by translation: CGSize, scale: CGFloat) {
        guard case .actual(let cx, let cy) = zoom, let stats = live?.stats(), stats.fullWidth > 0 else { return }
        if dragStart == nil { dragStart = (cx, cy) }
        let (sx, sy) = dragStart!
        zoom = .actual(
            centerX: sx - Float(translation.width * scale / CGFloat(stats.fullWidth)),
            centerY: sy - Float(translation.height * scale / CGFloat(stats.fullHeight)))
    }

    /// Ends a drag on the centre the core kept inside the photo.
    func endDrag() {
        dragStart = nil
        if let kept = live?.stats().zoom, case .actual = kept { zoom = kept }
    }
}

/// Stage 3: the photo drawn live by the core, with an Exposure slider.
struct LiveEditor: View {
    let photo: URL?
    @State private var model = LiveModel()
    @Environment(\.displayScale) private var displayScale

    var body: some View {
        VStack(spacing: 12) {
            GeometryReader { geometry in
                LiveViewRepresentable { [model] made in
                    model.live = made
                    if ProcessInfo.processInfo.arguments.contains("-sweep") { sweep(made) }
                    if ProcessInfo.processInfo.arguments.contains("-pan") { pan(made) }
                }
                .scaleEffect(model.pinch, anchor: model.pinchAnchor)
                .gesture(
                    MagnifyGesture()
                        .onChanged { value in
                            model.pinchAnchor = value.startAnchor
                            model.pinch = value.magnification
                        }
                        .onEnded { _ in model.endPinch(in: geometry.size, scale: displayScale) }
                )
                .simultaneousGesture(
                    DragGesture(minimumDistance: 4)
                        .onChanged { value in model.drag(by: value.translation, scale: displayScale) }
                        .onEnded { _ in model.endDrag() }
                )
                .onTapGesture(count: 2) { location in
                    model.toggleZoom(at: location, in: geometry.size, scale: displayScale)
                }
            }
            .background(Color(white: 0.11))
            .clipShape(RoundedRectangle(cornerRadius: 8))

            HStack(spacing: 16) {
                Text("Exposure").frame(width: 80, alignment: .leading)
                Slider(value: $model.exposure, in: -5...5)
                Text(String(format: "%+.2f", model.exposure))
                    .font(.body.monospacedDigit())
                    .frame(width: 56, alignment: .trailing)
                Picker("Decode", selection: $model.size) {
                    Text("Half").tag(DecodeSize.half)
                    Text("Full").tag(DecodeSize.full)
                }
                .pickerStyle(.segmented)
                .frame(width: 140)
            }

            TimelineView(.periodic(from: .now, by: 0.25)) { _ in
                Text(summary(model.live?.stats() ?? .empty))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding()
        .onAppear {
            // -full: decode at full size from the start, for -sweep.
            if ProcessInfo.processInfo.arguments.contains("-full") { model.size = .full }
            model.photo = photo
        }
        .onChange(of: photo) { _, new in model.photo = new }
    }

    private func summary(_ s: FrameStats) -> String {
        if let error = s.error { return "Error: \(error)" }
        if s.loading { return "Opening and decoding…" }
        if s.frames == 0 { return "Choose a photo." }
        let memory = Memory.footprint()
        let zoom = if case .actual = s.zoom { "100%" } else { "Fit" }
        return """
        \(s.camera) · decode \(Int(s.decodeMs)) ms · \(zoom) · \(s.backend) \(s.width)×\(s.height) of \(s.fullWidth)×\(s.fullHeight) · frames \(s.frames)
        render \(Int(s.renderMs)) ms (mean of 30: \(Int(s.meanRenderMs)) ms) · draw \(String(format: "%.1f", s.presentMs)) ms · memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak)), available \(Memory.mb(Memory.available()))
        """
    }

    /// Drags Exposure from -2 to +2 and back at 60 steps a second, for 4
    /// seconds, then prints what it cost: the `-sweep` launch option.
    private func sweep(_ live: LiveView) {
        Task.detached {
            while live.stats().frames == 0 {
                if let error = live.stats().error { print("SWEEP FAILED: \(error)"); return }
                try? await Task.sleep(for: .milliseconds(100))
            }
            let start = live.stats()
            let clock = ContinuousClock.now
            let steps = 240
            for i in 0..<steps {
                let phase = Double(i) / Double(steps)
                let value = Float(phase < 0.5 ? -2 + 8 * phase : 6 - 8 * phase)
                live.setExposure(stops: value)
                try? await Task.sleep(for: .milliseconds(16))
            }
            try? await Task.sleep(for: .milliseconds(500))
            let end = live.stats()
            let seconds = Double((ContinuousClock.now - clock).components.seconds)
            let frames = end.frames - start.frames
            let memory = Memory.footprint()
            print("""
            SWEEP \(end.camera) [\(end.backend) \(end.width)×\(end.height)]: \(steps) slider values in \(String(format: "%.1f", seconds)) s → \(frames) frames, \
            \(String(format: "%.1f", Double(frames) / max(seconds, 0.001))) fps; mean render \(Int(end.meanRenderMs)) ms, last draw \(String(format: "%.1f", end.presentMs)) ms; \
            decode \(Int(end.decodeMs)) ms; memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak)), available \(Memory.mb(Memory.available()))
            SWEEP DONE
            """)
        }
    }
}

extension LiveEditor {
    /// At 100%, pans across the photo's middle from 30% to 70% of its width
    /// and back at 60 steps a second for 4 seconds, then prints what it
    /// cost: the `-pan` launch option.
    func pan(_ live: LiveView) {
        Task.detached {
            while live.stats().frames == 0 {
                if let error = live.stats().error { print("PAN FAILED: \(error)"); return }
                try? await Task.sleep(for: .milliseconds(100))
            }
            live.setZoom(zoom: .actual(centerX: 0.3, centerY: 0.5))
            let first = live.stats().frames
            while live.stats().frames == first { try? await Task.sleep(for: .milliseconds(20)) }
            let opened = live.stats()
            let start = live.stats()
            let clock = ContinuousClock.now
            let steps = 240
            for i in 0..<steps {
                let phase = Double(i) / Double(steps)
                let x = Float(phase < 0.5 ? 0.3 + 0.8 * phase : 1.1 - 0.8 * phase)
                live.setZoom(zoom: .actual(centerX: x, centerY: 0.5))
                try? await Task.sleep(for: .milliseconds(16))
            }
            try? await Task.sleep(for: .milliseconds(500))
            let end = live.stats()
            let seconds = Double((ContinuousClock.now - clock).components.seconds)
            let frames = end.frames - start.frames
            let memory = Memory.footprint()
            print("""
            PAN \(end.camera) [\(end.backend) region \(end.width)×\(end.height) of \(end.fullWidth)×\(end.fullHeight)]: first 100% frame \(Int(opened.renderMs)) ms; \
            \(steps) positions in \(String(format: "%.1f", seconds)) s → \(frames) frames, \(String(format: "%.1f", Double(frames) / max(seconds, 0.001))) fps; \
            mean render \(Int(end.meanRenderMs)) ms; memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak)), available \(Memory.mb(Memory.available()))
            PAN DONE
            """)
        }
    }
}

extension FrameStats {
    static let empty = FrameStats(
        frames: 0, renderMs: 0, presentMs: 0, meanRenderMs: 0, backend: "", width: 0, height: 0,
        fullWidth: 0, fullHeight: 0, zoom: .fit, camera: "", decodeMs: 0, loading: false, error: nil)
}
