import SwiftUI

/// Over the photo while the Targeted Adjustment Tool is on: a drag up or down
/// moves the sliders that change the colour (or tone) where it started. The
/// photo is sampled as the drag starts; travel made before the sample is in
/// is applied when it arrives, so a quick flick still counts.
struct TargetedOverlay: View {
    let model: LiveModel
    let kind: TargetKind
    let size: CGSize
    /// Where the drag started, in view points.
    @State private var at: CGPoint?
    /// Points dragged up since it started.
    @State private var travel: CGFloat = 0
    /// The sample is in and the drag moves sliders.
    @State private var ready = false
    /// The finger is up before the sample arrived.
    @State private var released = false

    var body: some View {
        let rect = model.photoRect(in: size)
        ZStack(alignment: .topLeading) {
            Color.clear.contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if at == nil { start(at: drag.startLocation, in: rect) }
                        travel = -drag.translation.height
                        if ready { model.live?.dragTargeted(up: Float(travel), done: false); model.refresh() }
                    }
                    .onEnded { drag in
                        travel = -drag.translation.height
                        if ready { finish() } else { released = true }
                    })
            if let at {
                Image(systemName: "scope")
                    .font(.system(size: 26, weight: .light)).foregroundStyle(.white)
                    .shadow(color: .black.opacity(0.6), radius: 2)
                    .overlay(alignment: .top) {
                        Image(systemName: "arrow.up.and.down").font(.caption).foregroundStyle(.white)
                            .offset(y: -18)
                    }
                    .position(at)
                    .allowsHitTesting(false)
            }
        }
        .frame(width: size.width, height: size.height)
    }

    private func start(at point: CGPoint, in rect: CGRect) {
        at = point
        ready = false
        released = false
        model.targetedNote = nil
        let live = model.live, kind = kind
        let where_ = model.viewPoint(point, in: rect)
        Task {
            let result = await Task.detached { Result { try live?.startTargeted(kind: kind, at: where_) } }.value
            switch result {
            case .success(true):
                ready = true
                model.live?.dragTargeted(up: Float(travel), done: released)
                model.refresh()
                if released { reset() }
            case .success:
                model.targetedNote = "Nothing to adjust there: try a more colourful spot."
                reset()
            case .failure(RenderError.Failed(let message)):
                model.targetedNote = message
                reset()
            case .failure(let error):
                model.targetedNote = "\(error)"
                reset()
            }
        }
    }

    private func finish() {
        model.live?.dragTargeted(up: Float(travel), done: true)
        model.refresh()
        reset()
    }

    private func reset() {
        at = nil
        travel = 0
        ready = false
        released = false
    }
}

/// A button that turns the Targeted Adjustment Tool on for `kind`, or off.
struct TargetButton: View {
    let model: LiveModel
    let kind: TargetKind

    var body: some View {
        Button {
            model.targeted = model.targeted == kind ? nil : kind
            model.targetedNote = nil
        } label: {
            Image(systemName: "scope").foregroundStyle(.white).padding(6)
                .background(model.targeted == kind ? Theme.accent : Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Targeted adjustment")
    }
}

/// What the tool says while on: how to use it, or why the last drag moved nothing.
struct TargetedHint: View {
    let model: LiveModel

    var body: some View {
        if model.targeted != nil {
            Text(model.targetedNote ?? "Drag up or down on the photo, on the colour to change.")
                .font(.caption).foregroundStyle(Theme.secondaryText)
        }
    }
}
