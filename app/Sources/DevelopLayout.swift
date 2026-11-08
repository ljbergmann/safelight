import SwiftUI

/// What the editor's right-hand panel shows, chosen on the tool rail.
enum DevelopPane: String, CaseIterable {
    case edit, crop, masks, presets, history

    var title: String {
        switch self {
        case .edit: "Edit"
        case .crop: "Crop"
        case .masks: "Masks and Heal"
        case .presets: "Presets"
        case .history: "History"
        }
    }

    var symbol: String {
        switch self {
        case .edit: "slider.horizontal.3"
        case .crop: "crop.rotate"
        case .masks: "circle.dashed.inset.filled"
        case .presets: "camera.filters"
        case .history: "clock.arrow.circlepath"
        }
    }
}

/// The narrow column at the editor's right edge: a button per pane, and the
/// photo's rating and info at the bottom.
struct ToolRail: View {
    @Bindable var model: LiveModel
    let rating: Int32
    let flag: Int32
    let setRating: (Int32) -> Void
    let setFlag: (Int32) -> Void
    let showInfo: () -> Void
    @State private var ratingOpen = false

    var body: some View {
        VStack(spacing: 10) {
            ForEach(DevelopPane.allCases, id: \.self) { pane in
                Button { model.pane = pane } label: {
                    Image(systemName: pane.symbol)
                        .font(.system(size: 19))
                        .frame(width: 42, height: 42)
                        .foregroundStyle(model.pane == pane ? .white : .primary)
                        .background(model.pane == pane ? Theme.accent : .clear, in: RoundedRectangle(cornerRadius: 9))
                }
                .accessibilityLabel(pane.title)
            }
            Spacer()
            Button { ratingOpen = true } label: {
                Image(systemName: rating > 0 ? "star.fill" : flag == 1 ? "flag.fill" : "star")
                    .font(.system(size: 19)).frame(width: 42, height: 42)
            }
            .accessibilityLabel("Rating and flag")
            .popover(isPresented: $ratingOpen) {
                VStack(spacing: 14) {
                    HStack(spacing: 6) {
                        ForEach(1...5, id: \.self) { n in
                            Button { setRating(rating == n ? 0 : Int32(n)) } label: {
                                Image(systemName: n <= rating ? "star.fill" : "star").font(.title2)
                                    .foregroundStyle(n <= rating ? .yellow : .secondary)
                            }
                        }
                    }
                    HStack(spacing: 18) {
                        Button("Pick", systemImage: flag == 1 ? "flag.fill" : "flag") { setFlag(flag == 1 ? 0 : 1) }
                        Button("Reject", systemImage: flag == -1 ? "xmark.circle.fill" : "xmark.circle") { setFlag(flag == -1 ? 0 : -1) }
                            .tint(.red)
                    }
                }
                .padding()
                .presentationCompactAdaptation(.popover)
            }
            Button(action: showInfo) {
                Image(systemName: "info.circle").font(.system(size: 19)).frame(width: 42, height: 42)
            }
            .accessibilityLabel("Info")
        }
        .buttonStyle(.plain)
        .padding(.vertical, 12)
        .frame(width: 58)
        .frame(maxHeight: .infinity)
        .background(Theme.panel)
    }
}

/// The right-hand panel: the chosen pane's tools.
struct DevelopPanel: View {
    let model: LiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch model.pane {
                    case .edit: EditPane(model: model)
                    case .crop: CropPane(model: model)
                    case .masks: MasksPanel(model: model).padding(.horizontal, 16).padding(.vertical, 12)
                    case .presets: PresetsPanel(model: model).padding(.horizontal, 16).padding(.vertical, 12)
                    case .history:
                        VStack(alignment: .leading, spacing: 12) {
                            SnapshotsPanel(model: model)
                            Divider().overlay(Theme.line)
                            HistoryPanel(model: model)
                        }
                        .padding(.horizontal, 16).padding(.vertical, 12)
                    }
                }
            }
            if let note = model.pasteNote {
                Text(note).font(.caption).foregroundStyle(.orange).padding(12)
            }
        }
        .frame(width: 340)
        .frame(maxHeight: .infinity)
        .background(Theme.raised)
    }
}

/// Edit: the histogram, then Basic's profile, Auto and white balance, then
/// every panel the core describes, each folding.
struct EditPane: View {
    let model: LiveModel
    @State private var open: Set<String> = ["Basic"]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HistogramPanel(model: model).padding(.horizontal, 16).padding(.top, 12)
            ForEach(model.edit?.sections ?? [], id: \.title) { section in
                PanelSection(section: section, model: model, isOpen: Binding(
                    get: { open.contains(section.title) },
                    set: { if $0 { open.insert(section.title) } else { open.remove(section.title) } }))
                if section.title == "AI Denoise" {
                    AiDenoiseStatus(model: model).padding(.horizontal, 16)
                }
            }
        }
    }
}

/// Crop: the ratio, rotate and flip, and Reset; the photo shows the crop's
/// handles while this pane is open.
struct CropPane: View {
    let model: LiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Aspect ratio").font(.headline)
            Menu {
                ForEach(CropAspect.presets, id: \.0) { name, aspect in
                    Button(name) { model.chooseAspect(aspect) }
                }
            } label: {
                HStack {
                    Image(systemName: model.cropAspect == .free ? "lock.open" : "lock")
                    Text(CropAspect.presets.first { $0.1 == model.cropAspect }?.0 ?? "Custom")
                    Spacer()
                    Image(systemName: "chevron.up.chevron.down").font(.caption)
                }
                .padding(10).background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
            }
            Button("Swap Orientation", systemImage: "rectangle.portrait.rotate") { model.swapCropOrientation() }
                .disabled(model.cropAspect == .free)
            Divider().overlay(Theme.line)
            Text("Rotate and flip").font(.headline)
            HStack(spacing: 22) {
                Button("Rotate Left", systemImage: "rotate.left") { model.orient(.rotateLeft) }
                Button("Rotate Right", systemImage: "rotate.right") { model.orient(.rotateRight) }
                Button("Flip", systemImage: "arrow.left.and.right.righttriangle.left.righttriangle.right") { model.orient(.flipHorizontal) }
                Button("Flip Vertical", systemImage: "arrow.up.and.down.righttriangle.up.righttriangle.down") { model.orient(.flipVertical) }
            }
            .labelStyle(.iconOnly).font(.title2)
            Divider().overlay(Theme.line)
            Button("Reset Crop", systemImage: "arrow.counterclockwise") { model.setCrop(CropRect(l: 0, t: 0, r: 1, b: 1), done: true) }
        }
        .padding(16)
    }
}

/// A slider as the panel shows it: a thin track and a small knob. A drag
/// moves the value by how far the finger goes, wherever it starts, so a
/// touch never jumps it; one drag is one History step.
struct ThinSlider: View {
    let value: Float
    let range: ClosedRange<Float>
    let changed: (Float, Bool) -> Void
    @State private var start: Float?

    private static let knob: CGFloat = 16

    var body: some View {
        GeometryReader { geometry in
            let span = range.upperBound - range.lowerBound
            let travel = max(geometry.size.width - Self.knob, 1)
            let at = span > 0 ? CGFloat((value - range.lowerBound) / span) : 0
            ZStack(alignment: .leading) {
                Capsule().fill(Theme.line).frame(height: 2)
                Circle().fill(.white).frame(width: Self.knob, height: Self.knob)
                    .offset(x: min(max(at, 0), 1) * travel)
            }
            .frame(maxHeight: .infinity)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { drag in
                        if start == nil { start = value }
                        changed(moved(drag.translation.width, travel: travel, span: span), false)
                    }
                    .onEnded { drag in
                        changed(moved(drag.translation.width, travel: travel, span: span), true)
                        start = nil
                    }
            )
        }
        .frame(height: 26)
    }

    private func moved(_ dx: CGFloat, travel: CGFloat, span: Float) -> Float {
        let base = start ?? value
        return min(max(base + Float(dx / travel) * span, range.lowerBound), range.upperBound)
    }
}
