import SwiftUI

extension PhotoTool {
    /// Tools that take one point of the photo: white balance, and a Color or
    /// Luminance Range mask, new or added to the selected mask. They sample
    /// through [`SampleLoupe`], placed and then confirmed, as Lightroom's are.
    var samples: Bool {
        switch self {
        case .whiteBalance, .pointColor, .newGradient(.colorRange), .newGradient(.luminanceRange),
             .newPart(.colorRange, _), .newPart(.luminanceRange, _):
            true
        default:
            false
        }
    }
}

extension LiveModel {
    /// Uses the point `at` (fractions across the photo as shown) for the
    /// sampling tool open, and closes it.
    func sample(at point: [Float]) {
        switch tool {
        case .whiteBalance:
            live?.pickWhiteBalance(at: point)
            tool = .none
            refresh()
        case .pointColor:
            tool = .none
            pointColorNote = "Sampling…"
            let live = live
            Task {
                let result = await Task.detached { Result { try live?.addPointColor(at: point) } }.value
                switch result {
                case .success(let index):
                    pointColorSelected = index
                    pointColorNote = nil
                case .failure(RenderError.Failed(let message)):
                    pointColorNote = message
                case .failure(let error):
                    pointColorNote = "\(error)"
                }
                refresh()
            }
        case .newGradient(let kind):
            addMask(kind, from: point, to: point)
        case .newPart(let kind, let op):
            if let mask = selectedMask {
                _ = live?.addMaskComponent(mask: mask, kind: kind, operation: op, from: point, to: point)
            }
            tool = .none
            refresh()
        default:
            break
        }
    }
}

/// A ring with a crosshair over the photo, for a tool that samples one
/// point: drag it (or tap the photo to move it there), then tap ✓ to use
/// the point under the crosshair.
struct SampleLoupe: View {
    let model: LiveModel
    let size: CGSize
    /// Where the crosshair is, as fractions across the photo.
    @State private var at = CGPoint(x: 0.5, y: 0.5)
    /// `at` when the drag began.
    @State private var start: CGPoint?

    private static let ring: CGFloat = 110

    var body: some View {
        let rect = model.photoRect(in: size)
        let center = CGPoint(x: rect.minX + at.x * rect.width, y: rect.minY + at.y * rect.height)
        let r = Self.ring / 2
        ZStack(alignment: .topLeading) {
            // The photo takes the touches: a drag moves the ring by as much,
            // a tap puts it there.
            Color.clear.contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        let from = start ?? at
                        if start == nil { start = at }
                        let moved = hypot(drag.translation.width, drag.translation.height) > 4
                        let target = moved
                            ? CGPoint(x: from.x + drag.translation.width / rect.width,
                                      y: from.y + drag.translation.height / rect.height)
                            : from
                        at = Self.clamped(target)
                    }
                    .onEnded { drag in
                        if hypot(drag.translation.width, drag.translation.height) <= 4 {
                            at = Self.clamped(CGPoint(x: (drag.location.x - rect.minX) / rect.width,
                                                      y: (drag.location.y - rect.minY) / rect.height))
                        }
                        start = nil
                    })
            Circle()
                .strokeBorder(.white, lineWidth: 2.5)
                .shadow(color: .black.opacity(0.5), radius: 2)
                .frame(width: Self.ring, height: Self.ring)
                .overlay {
                    Image(systemName: "plus").font(.system(size: 14, weight: .light)).foregroundStyle(.white)
                        .shadow(color: .black.opacity(0.6), radius: 1)
                }
                .position(center)
                .allowsHitTesting(false)
            Button {
                model.sample(at: [Float(at.x), Float(at.y)])
            } label: {
                Image(systemName: "checkmark")
                    .font(.system(size: 15, weight: .bold)).foregroundStyle(.white)
                    .frame(width: 32, height: 32)
                    .background(Theme.accent, in: Circle())
                    .overlay(Circle().strokeBorder(.white, lineWidth: 1.5))
            }
            .buttonStyle(.plain)
            // On the ring, up and to the right, or on whichever side keeps it
            // within the view near an edge.
            .position(Self.check(center, r, in: size))
            .accessibilityLabel("Use this point")
        }
        .frame(width: size.width, height: size.height)
    }

    private static func check(_ c: CGPoint, _ r: CGFloat, in size: CGSize) -> CGPoint {
        let d = r * 0.71, room: CGFloat = 20
        let x = c.x + d + room > size.width ? c.x - d : c.x + d
        let y = c.y - d - room < 0 ? c.y + d : c.y - d
        return CGPoint(x: x, y: y)
    }

    private static func clamped(_ p: CGPoint) -> CGPoint {
        CGPoint(x: min(max(p.x, 0), 1), y: min(max(p.y, 0), 1))
    }
}
