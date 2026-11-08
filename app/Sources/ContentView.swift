import SwiftUI

/// Stage 2: develops one RAW from the app's Documents folder through the Rust
/// core, headless, and shows the result with what each step cost.
struct ContentView: View {
    @State private var photos: [URL] = []
    @State private var selected: URL?
    @State private var backend: Backend = .gpu
    @State private var size: DecodeSize = .full
    @State private var running = false
    @State private var result: Result<Measured, Error>?

    var body: some View {
        NavigationSplitView {
            List(photos, id: \.self, selection: $selected) { url in
                Text(url.lastPathComponent)
            }
            .navigationTitle("Documents")
            .overlay {
                if photos.isEmpty {
                    ContentUnavailableView(
                        "No photos",
                        systemImage: "photo",
                        description: Text("Copy RAW files into the app's Documents folder (Finder, or the Files app).")
                    )
                }
            }
            .refreshable { photos = Documents.photos() }
        } detail: {
            VStack(spacing: 16) {
                controls
                Divider()
                output
                Spacer(minLength: 0)
            }
            .padding()
            .navigationTitle(selected?.lastPathComponent ?? "RAWmakase")
        }
        .onAppear {
            photos = Documents.photos()
            if ProcessInfo.processInfo.arguments.contains("-benchmark") { benchmark() }
        }
    }

    private var controls: some View {
        HStack(spacing: 16) {
            Picker("Renderer", selection: $backend) {
                Text("GPU").tag(Backend.gpu)
                Text("CPU").tag(Backend.cpu)
            }
            .pickerStyle(.segmented)
            .frame(maxWidth: 180)
            Picker("Decode", selection: $size) {
                Text("Full size").tag(DecodeSize.full)
                Text("Half size").tag(DecodeSize.half)
            }
            .pickerStyle(.segmented)
            .frame(maxWidth: 240)
            Spacer()
            Button("Render") { render() }
                .buttonStyle(.borderedProminent)
                .disabled(selected == nil || running)
            if running { ProgressView() }
        }
    }

    @ViewBuilder private var output: some View {
        switch result {
        case .none:
            Text("Choose a photo and render it.").foregroundStyle(.secondary)
        case .failure(let error):
            Text(String(describing: error)).foregroundStyle(.red).textSelection(.enabled)
        case .success(let m):
            Image(decorative: m.image, scale: 1)
                .resizable()
                .scaledToFit()
                .frame(maxHeight: 640)
            Text(m.summary)
                .font(.callout.monospaced())
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    /// Renders every photo in each mode, one after another, printing a line
    /// per render and writing them to Documents/benchmark.txt: the launch
    /// option `-benchmark` runs it, for measuring without tapping.
    private func benchmark() {
        running = true
        let photos = photos
        Task.detached(priority: .userInitiated) {
            var lines: [String] = []
            let modes: [(Backend, DecodeSize, String)] = [
                (.gpu, .full, "GPU full"), (.cpu, .full, "CPU full"), (.gpu, .half, "GPU half"),
            ]
            for url in photos {
                for (backend, size, label) in modes {
                    let before = Memory.footprint()
                    let line: String
                    do {
                        let r = try renderPhoto(path: url.path, maxEdge: 2048, size: size, backend: backend)
                        let m = Measured(r, before: before, after: Memory.footprint())
                        if backend == .gpu && size == .full {
                            try Documents.writePPM(r, named: url.lastPathComponent + ".ipad.ppm")
                        }
                        await MainActor.run { result = .success(m) }
                        line = "\(url.lastPathComponent) [\(label)]\n\(m.summary)"
                    } catch {
                        line = "\(url.lastPathComponent) [\(label)] failed: \(error)"
                    }
                    print(line, "\n")
                    lines.append(line)
                }
            }
            try? lines.joined(separator: "\n\n").write(
                to: Documents.url.appending(path: "benchmark.txt"), atomically: true, encoding: .utf8)
            print("BENCHMARK DONE")
            await MainActor.run { running = false }
        }
    }

    private func render() {
        guard let url = selected else { return }
        running = true
        let (backend, size) = (backend, size)
        Task.detached(priority: .userInitiated) {
            let before = Memory.footprint()
            let outcome = Result {
                let r = try renderPhoto(path: url.path, maxEdge: 2048, size: size, backend: backend)
                let after = Memory.footprint()
                try Documents.writePPM(r, named: url.lastPathComponent + ".ipad.ppm")
                return Measured(r, before: before, after: after)
            }
            await MainActor.run {
                result = outcome
                running = false
            }
        }
    }
}

/// A rendering with the app's memory around it.
struct Measured: @unchecked Sendable {
    let image: CGImage
    let summary: String

    init(_ r: Rendering, before: Memory.Footprint, after: Memory.Footprint) {
        image = Pixels.image(rgba: r.rgba, width: Int(r.width), height: Int(r.height))
        let total = r.openMs + r.resolveMs + r.decodeMs + r.rendererMs + r.renderMs
        summary = """
        \(r.camera) · decoded \(r.decodedWidth)×\(r.decodedHeight) · shown \(r.width)×\(r.height) · \(r.backend)
        open \(Int(r.openMs)) ms · resolve \(Int(r.resolveMs)) ms · decode \(Int(r.decodeMs)) ms · renderer \(Int(r.rendererMs)) ms · render \(Int(r.renderMs)) ms · total \(Int(total)) ms
        memory before \(Memory.mb(before.current)) · after \(Memory.mb(after.current)) · peak \(Memory.mb(after.peak)) · available now \(Memory.mb(Memory.available()))
        """
    }
}
