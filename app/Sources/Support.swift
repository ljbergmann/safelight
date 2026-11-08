import CoreGraphics
import Foundation
import os

/// The app's Documents folder, which Finder and the Files app can reach.
enum Documents {
    static var url: URL { URL.documentsDirectory }

    static let rawExtensions: Set<String> = ["dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2", "pef", "srw"]

    static func photos() -> [URL] {
        let files = (try? FileManager.default.contentsOfDirectory(at: url, includingPropertiesForKeys: nil)) ?? []
        return files
            .filter { rawExtensions.contains($0.pathExtension.lowercased()) }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
    }

    /// The rendered pixels as a binary PPM, to compare with the Mac's render.
    static func writePPM(_ r: Rendering, named name: String) throws {
        var data = Data("P6\n\(r.width) \(r.height)\n255\n".utf8)
        data.reserveCapacity(data.count + Int(r.width * r.height * 3))
        r.rgba.withUnsafeBytes { bytes in
            for i in stride(from: 0, to: bytes.count, by: 4) {
                data.append(contentsOf: [bytes[i], bytes[i + 1], bytes[i + 2]])
            }
        }
        try data.write(to: url.appending(path: name))
    }
}

/// RGBA bytes as an image.
enum Pixels {
    static func image(rgba: Data, width: Int, height: Int) -> CGImage {
        let provider = CGDataProvider(data: rgba as CFData)!
        return CGImage(
            width: width, height: height,
            bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: width * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
        )!
    }
}

/// The app's memory as iPadOS counts it: what the system kills an app for.
enum Memory {
    struct Footprint: Sendable {
        var current: UInt64
        var peak: UInt64
    }

    static func footprint() -> Footprint {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self(), task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        guard result == KERN_SUCCESS else { return Footprint(current: 0, peak: 0) }
        return Footprint(current: info.phys_footprint, peak: UInt64(max(0, info.ledger_phys_footprint_peak)))
    }

    /// How much more the app may use before iPadOS ends it; 0 in the simulator.
    static func available() -> UInt64 { UInt64(os_proc_available_memory()) }

    static func mb(_ bytes: UInt64) -> String { "\(bytes / 1_048_576) MB" }
}

private func mach_task_self() -> mach_port_t { mach_task_self_ }
