import QuartzCore
import SwiftUI
import UIKit

/// A view whose layer is a `CAMetalLayer`, drawn into by the Rust core's
/// `LiveView` on its own thread.
final class MetalLayerView: UIView {
    override class var layerClass: AnyClass { CAMetalLayer.self }
    var metalLayer: CAMetalLayer { layer as! CAMetalLayer }

    /// Created on the first layout with a size; closed when the view goes.
    private(set) var live: LiveView?
    var onLive: ((LiveView) -> Void)?

    override func layoutSubviews() {
        super.layoutSubviews()
        let scale = window?.screen.scale ?? traitCollection.displayScale
        contentScaleFactor = scale
        let width = UInt32(max(1, bounds.width * scale))
        let height = UInt32(max(1, bounds.height * scale))
        if let live {
            live.resize(width: width, height: height)
        } else if bounds.width > 0, bounds.height > 0 {
            let pointer = UInt64(UInt(bitPattern: Unmanaged.passUnretained(metalLayer).toOpaque()))
            if let made = try? LiveView(layer: pointer, width: width, height: height) {
                live = made
                onLive?(made)
            }
        }
    }

    /// Stops the core's worker before the layer can go away.
    func close() {
        live?.close()
        live = nil
    }
}

struct LiveViewRepresentable: UIViewRepresentable {
    let onLive: (LiveView) -> Void

    func makeUIView(context: Context) -> MetalLayerView {
        let view = MetalLayerView()
        view.onLive = onLive
        return view
    }

    func updateUIView(_ view: MetalLayerView, context: Context) {}

    static func dismantleUIView(_ view: MetalLayerView, coordinator: ()) {
        view.close()
    }
}
