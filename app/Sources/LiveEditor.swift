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

    private func open() {
        guard let live, let photo else { return }
        exposure = 0
        live.open(path: photo.path, size: size)
    }
}

/// Stage 3: the photo drawn live by the core, with an Exposure slider.
struct LiveEditor: View {
    let photo: URL?
    @State private var model = LiveModel()

    var body: some View {
        VStack(spacing: 12) {
            LiveViewRepresentable { [model] made in
                model.live = made
                if ProcessInfo.processInfo.arguments.contains("-sweep") { sweep(made) }
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
        return """
        \(s.camera) · decode \(Int(s.decodeMs)) ms · \(s.backend) \(s.width)×\(s.height) · frames \(s.frames)
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

extension FrameStats {
    static let empty = FrameStats(
        frames: 0, renderMs: 0, presentMs: 0, meanRenderMs: 0, backend: "", width: 0, height: 0,
        camera: "", decodeMs: 0, loading: false, error: nil)
}
