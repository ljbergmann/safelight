import SwiftUI

/// Lightroom's Before/After menu: the views, and copying or swapping
/// settings between Before and the edit. Choosing the view on screen again
/// goes back to the edit alone, as Lightroom's keys do.
struct BeforeAfterMenu: View {
    let model: LiveModel

    private static let views: [(String, Compare)] = [
        ("Before Only", .beforeOnly),
        ("Left / Right", .sideBySide(topBottom: false)),
        ("Top / Bottom", .sideBySide(topBottom: true)),
        ("Split Left / Right", .split(topBottom: false, position: 0.5)),
        ("Split Top / Bottom", .split(topBottom: true, position: 0.5)),
    ]

    var body: some View {
        Menu {
            Button("Edit Only") { model.compare = .off }
            ForEach(Self.views, id: \.0) { name, view in
                Button {
                    model.compare = model.compare.sameView(as: view) ? .off : view
                    if model.compare != .off { model.cropping = false; model.tool = .none }
                } label: {
                    if model.compare.sameView(as: view) { Label(name, systemImage: "checkmark") } else { Text(name) }
                }
            }
            Divider()
            Button("Copy Before Settings to After") { copy(.beforeToAfter) }
            Button("Copy After Settings to Before") { copy(.afterToBefore) }
            Button("Swap Before and After Settings") { copy(.swap) }
        } label: {
            Label("Before/After", systemImage: model.compare == .off ? "rectangle.split.2x1" : "rectangle.split.2x1.fill")
        }
        .disabled(model.edit == nil)
    }

    private func copy(_ copy: BeforeAfterCopy) {
        model.live?.copyBeforeAfter(copy: copy)
        model.refresh()
    }
}

extension Compare {
    /// The same view, wherever a split's line is.
    func sameView(as other: Compare) -> Bool {
        switch (self, other) {
        case (.split(let a, _), .split(let b, _)): a == b
        default: self == other
        }
    }
}

/// Over the photo in a Before/After view: which side is which, and a split
/// view's line, which a drag moves.
struct CompareOverlay: View {
    let model: LiveModel
    let size: CGSize

    var body: some View {
        ZStack(alignment: .topLeading) {
            switch model.compare {
            case .beforeOnly:
                tag("Before").padding(8)
            case .sideBySide(let topBottom):
                tag("Before").padding(8)
                tag("After").padding(8)
                    .frame(width: size.width, height: size.height,
                           alignment: topBottom ? .bottomLeading : .topTrailing)
            case .split(let topBottom, let position):
                splitLine(topBottom: topBottom, position: CGFloat(position))
            case .off:
                EmptyView()
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
        .coordinateSpace(name: "compare")
    }

    private func tag(_ text: String) -> some View {
        Text(text).font(.caption.bold()).padding(.horizontal, 6).padding(.vertical, 2)
            .background(.black.opacity(0.55), in: Capsule()).foregroundStyle(.white)
            .allowsHitTesting(false)
    }

    private func splitLine(topBottom: Bool, position: CGFloat) -> some View {
        let at = topBottom ? size.height * position : size.width * position
        return ZStack(alignment: .topLeading) {
            Rectangle().fill(.white.opacity(0.9))
                .frame(width: topBottom ? size.width : 2, height: topBottom ? 2 : size.height)
                .offset(x: topBottom ? 0 : at - 1, y: topBottom ? at - 1 : 0)
                .allowsHitTesting(false)
            // A wide handle on the line, for a finger.
            Circle().fill(.white).frame(width: 28, height: 28)
                .overlay(Image(systemName: topBottom ? "arrow.up.and.down" : "arrow.left.and.right")
                    .font(.caption.bold()).foregroundStyle(.black))
                .offset(x: topBottom ? size.width / 2 - 14 : at - 14, y: topBottom ? at - 14 : size.height / 2 - 14)
                .gesture(DragGesture(minimumDistance: 0, coordinateSpace: .named("compare")).onChanged { value in
                    let moved = topBottom ? value.location.y / size.height : value.location.x / size.width
                    model.compare = .split(topBottom: topBottom, position: Float(min(max(moved, 0.02), 0.98)))
                })
            tag("Before").padding(8)
            tag("After").padding(8)
                .frame(width: size.width, height: size.height, alignment: topBottom ? .bottomLeading : .topTrailing)
        }
    }
}
