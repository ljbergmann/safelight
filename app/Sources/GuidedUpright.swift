import SwiftUI

/// Guided Upright's guides on the photo, as the desktop's Guided tool draws
/// them: drag across the photo to add a guide (up to four), drag an end to
/// move it, tap a guide to select it for Delete. Each change is one History
/// step, solved by the core at once.
struct GuidesOverlay: View {
    let model: LiveModel
    let size: CGSize

    private static let maxGuides = 4
    /// How near an end a touch must start to move it, in points.
    private static let grab: CGFloat = 28

    private enum Drag {
        case new(from: CGPoint, to: CGPoint)
        case end(guide: Int, end: Int, to: CGPoint)
    }

    @State private var drag: Drag?
    @State private var selected: Int?
    @State private var note: String?
    @State private var working = false

    var body: some View {
        let _ = model.edit?.applied
        let _ = model.edit?.steps.count
        let rect = model.photoRect(in: size)
        let guides = shownGuides(in: rect)
        ZStack(alignment: .topLeading) {
            Color.clear.contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in changed(value, guides: guides) }
                        .onEnded { value in ended(value, guides: guides, rect: rect) }
                )
            ForEach(guides.indices, id: \.self) { i in
                line(guides[i], selected: selected == i)
            }
            if case .new(let from, let to) = drag {
                line([from, to], selected: true)
            }
        }
        .overlay(alignment: .top) { bar(count: guides.count).padding(.top, 12) }
        .overlay(alignment: .bottom) {
            if let note {
                Text(note).font(.callout)
                    .padding(.horizontal, 12).padding(.vertical, 6)
                    .background(.regularMaterial, in: Capsule())
                    .padding(.bottom, 16)
            }
        }
    }

    private func bar(count: Int) -> some View {
        HStack(spacing: 14) {
            if working { ProgressView().controlSize(.small) }
            Text(count == 0 ? "Drag along a line that should be straight" : "\(count) of \(Self.maxGuides) guides")
                .font(.callout).foregroundStyle(Theme.secondaryText)
            Button("Delete Guide", systemImage: "trash") { delete() }
                .disabled(selected == nil || working)
            Button("Clear") { set([], step: "Clear Guides") }
                .disabled(count == 0 || working)
            Button("Done") { model.guiding = false }
                .bold()
        }
        .padding(.horizontal, 14).padding(.vertical, 8)
        .background(.regularMaterial, in: Capsule())
    }

    private func line(_ ends: [CGPoint], selected: Bool) -> some View {
        ZStack {
            Path { p in p.move(to: ends[0]); p.addLine(to: ends[1]) }
                .stroke(selected ? Color.yellow : .white, lineWidth: selected ? 2 : 1.5)
                .shadow(color: .black.opacity(0.7), radius: 1)
            ForEach(0..<2, id: \.self) { e in
                Circle().fill(.white).frame(width: 10, height: 10)
                    .overlay(Circle().stroke(.black.opacity(0.6), lineWidth: 1))
                    .position(ends[e])
            }
        }
        .allowsHitTesting(false)
    }

    /// The guides in view points, with the end being dragged where it is.
    private func shownGuides(in rect: CGRect) -> [[CGPoint]] {
        let flat = model.live?.uprightGuides() ?? []
        var guides = stride(from: 0, to: flat.count - 3, by: 4).map { i in
            [point(flat[i], flat[i + 1], rect), point(flat[i + 2], flat[i + 3], rect)]
        }
        if case .end(let g, let e, let to) = drag, g < guides.count {
            guides[g][e] = to
        }
        return guides
    }

    private func point(_ x: Float, _ y: Float, _ rect: CGRect) -> CGPoint {
        CGPoint(x: rect.minX + CGFloat(x) * rect.width, y: rect.minY + CGFloat(y) * rect.height)
    }

    private func changed(_ value: DragGesture.Value, guides: [[CGPoint]]) {
        guard !working else { return }
        switch drag {
        case nil:
            let start = value.startLocation
            var nearest: (Int, Int, CGFloat)?
            for (g, ends) in guides.enumerated() {
                for (e, p) in ends.enumerated() {
                    let d = hypot(p.x - start.x, p.y - start.y)
                    if d < Self.grab && d < (nearest?.2 ?? .infinity) { nearest = (g, e, d) }
                }
            }
            if let (g, e, _) = nearest {
                drag = .end(guide: g, end: e, to: value.location)
                selected = g
            } else {
                drag = .new(from: start, to: value.location)
            }
        case .new(let from, _):
            drag = .new(from: from, to: value.location)
        case .end(let g, let e, _):
            drag = .end(guide: g, end: e, to: value.location)
        }
    }

    private func ended(_ value: DragGesture.Value, guides: [[CGPoint]], rect: CGRect) {
        defer { drag = nil }
        guard !working else { return }
        let moved = hypot(value.translation.width, value.translation.height)
        switch drag {
        case .new(let from, let to) where moved >= 12:
            guard guides.count < Self.maxGuides else {
                note = "Guided Upright takes at most \(Self.maxGuides) guides"
                return
            }
            selected = guides.count
            set(guides + [[from, to]], step: "Add Guide", in: rect)
        case .new:
            // A tap: select the guide under it.
            selected = guides.firstIndex { distance(value.location, to: $0) < 16 }
        case .end(let g, _, _) where moved >= 2:
            set(guides, step: "Move Guide", in: rect)
            selected = g
        default:
            break
        }
    }

    private func delete() {
        guard let i = selected else { return }
        let rect = model.photoRect(in: size)
        var guides = shownGuides(in: rect)
        guard i < guides.count else { return }
        guides.remove(at: i)
        selected = nil
        set(guides, step: "Delete Guide", in: rect)
    }

    /// Distance from a point to a segment.
    private func distance(_ p: CGPoint, to ends: [CGPoint]) -> CGFloat {
        let (a, b) = (ends[0], ends[1])
        let (dx, dy) = (b.x - a.x, b.y - a.y)
        let t = max(0, min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / max(dx * dx + dy * dy, 1)))
        return hypot(a.x + t * dx - p.x, a.y + t * dy - p.y)
    }

    private func set(_ guides: [[CGPoint]], step: String, in rect: CGRect? = nil) {
        let rect = rect ?? model.photoRect(in: size)
        let points = guides.flatMap { $0 }.flatMap { model.viewPoint($0, in: rect) }
        let live = model.live
        working = true
        Task {
            let result = await Task.detached { Result { try live?.setUprightGuides(points: points, step: step) } }.value
            switch result {
            case .success(let said): note = said ?? nil
            case .failure(RenderError.Failed(let message)): note = message
            case .failure(let error): note = "\(error)"
            }
            model.refresh()
            working = false
        }
    }
}
