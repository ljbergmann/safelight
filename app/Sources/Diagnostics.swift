import Foundation

/// Measurements started by launch options.
enum Diagnostics {
    /// Renders every photo in each mode, one after another, printing a line
    /// per render and writing them to Documents/benchmark.txt: the launch
    /// option `-benchmark` runs it, for measuring without tapping.
    static func benchmark() {
        let photos = Documents.photos()
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
        }
    }
}

/// What a one-off render cost, with the app's memory around it.
struct Measured: Sendable {
    let summary: String

    init(_ r: Rendering, before: Memory.Footprint, after: Memory.Footprint) {
        let total = r.openMs + r.resolveMs + r.decodeMs + r.rendererMs + r.renderMs
        summary = """
        \(r.camera) · decoded \(r.decodedWidth)×\(r.decodedHeight) · shown \(r.width)×\(r.height) · \(r.backend)
        open \(Int(r.openMs)) ms · resolve \(Int(r.resolveMs)) ms · decode \(Int(r.decodeMs)) ms · renderer \(Int(r.rendererMs)) ms · render \(Int(r.renderMs)) ms · total \(Int(total)) ms
        memory before \(Memory.mb(before.current)) · after \(Memory.mb(after.current)) · peak \(Memory.mb(after.peak)) · available now \(Memory.mb(Memory.available()))
        """
    }
}
