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

/// Edit, as Lightroom for iPad lays it out: Auto, the profile, then the
/// groups Light, Color, Effects, Detail and Optics, each folding.
struct EditPane: View {
    let model: LiveModel
    @State private var open: Set<String> = ["Light"]

    static let groups = ["Light", "Color", "Effects", "Detail", "Optics"]

    var body: some View {
        let sections = model.edit?.sections ?? []
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Edit").font(.title3.weight(.semibold))
                Spacer()
                Button {
                    model.runAuto(.settings)
                } label: {
                    if model.autoRunning == .settings {
                        ProgressView().controlSize(.small)
                    } else {
                        Label("Auto", systemImage: "wand.and.stars")
                    }
                }
                .buttonStyle(.bordered)
                .disabled(model.autoRunning != nil || model.edit == nil)
            }
            .padding(16)
            rule
            ProfileRow(model: model, amount: sections.first { $0.group == "Profile" })
            rule
            ForEach(Self.groups, id: \.self) { name in
                EditGroup(
                    name: name, sections: sections.filter { $0.group == name }, model: model,
                    isOpen: Binding(
                        get: { open.contains(name) },
                        set: { if $0 { open.insert(name) } else { open.remove(name) } }))
            }
        }
    }

    private var rule: some View { Rectangle().fill(Theme.line).frame(height: 0.5) }
}

/// The profile and Browse, which lists the profiles this photo can use;
/// the profile's Amount below it when it has one.
struct ProfileRow: View {
    let model: LiveModel
    let amount: Section?

    var body: some View {
        let profiles = model.edit == nil ? [] : (model.live?.profiles() ?? [])
        let top = profiles.filter { $0.group.isEmpty }
        let groups = Array(Set(profiles.map(\.group).filter { !$0.isEmpty }))
            .sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Profile").font(.headline)
                    Text(profiles.first(where: \.selected)?.title ?? "—")
                        .font(.callout).foregroundStyle(Theme.secondaryText).lineLimit(1)
                }
                Spacer()
                Menu {
                    ForEach(top, id: \.title) { item($0) }
                    ForEach(groups, id: \.self) { group in
                        Menu(group) {
                            ForEach(profiles.filter { $0.group == group }, id: \.title) { item($0) }
                        }
                    }
                } label: {
                    Text("Browse").padding(.horizontal, 12).padding(.vertical, 7)
                        .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
            }
            ForEach(amount?.sliders ?? [], id: \.label) { SliderRow(slider: $0, model: model) }
        }
        .padding(16)
    }

    private func item(_ p: ProfileInfo) -> some View {
        Button {
            model.live?.setProfile(index: p.index)
            model.refresh()
        } label: {
            if p.selected { Label(p.title, systemImage: "checkmark") } else { Text(p.title) }
        }
    }
}

/// One group of the Edit pane. Its first section's controls show when it
/// opens; the rest are opened from it: Light's curve from the curve button,
/// Color's mixer, grading and calibration from buttons below its sliders;
/// Effects', Detail's and Optics' follow under their own titles.
struct EditGroup: View {
    let name: String
    let sections: [Section]
    let model: LiveModel
    @Binding var isOpen: Bool
    /// The families opened from buttons ("Tone Curve", "Color Mixer"…).
    @State private var shown: Set<String> = []

    private var main: Section? { sections.first { $0.title == name } ?? sections.first }
    private var rest: [Section] { sections.filter { $0.title != main?.title } }

    /// "Color Mixer" for "Color Mixer · Hue".
    private static func family(_ title: String) -> String {
        title.components(separatedBy: " · ").first ?? title
    }

    /// Families opened from a button rather than shown under their title.
    private var buttoned: [String] {
        guard name == "Light" || name == "Color" else { return [] }
        var order: [String] = []
        for s in rest where !order.contains(Self.family(s.title)) { order.append(Self.family(s.title)) }
        return order
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Button {
                    withAnimation(.snappy(duration: 0.2)) { isOpen.toggle() }
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: "chevron.right")
                            .font(.footnote.weight(.semibold))
                            .rotationEffect(.degrees(isOpen ? 90 : 0))
                        Text(name).font(.headline)
                        Spacer()
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                if name == "Light" {
                    toggleButton("Tone Curve", label: Image(systemName: "point.topleft.down.to.point.bottomright.curvepath"))
                }
                if name == "Color", let bw = main?.switches.first(where: { $0.id == .monochrome }) {
                    Button { model.setSwitch(.monochrome, on: !bw.on) } label: {
                        Text("B&W").font(.callout)
                            .padding(.horizontal, 10).padding(.vertical, 6)
                            .background(bw.on ? Theme.accent : Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 16).padding(.vertical, 14)
            if isOpen, let main {
                VStack(alignment: .leading, spacing: 6) {
                    if name == "Color" { WhiteBalanceRow(model: model) }
                    controls(main, skipping: [.monochrome])
                    if name == "Light", shown.contains("Tone Curve") {
                        subsections("Tone Curve")
                    }
                    if name == "Color" {
                        ForEach(buttoned, id: \.self) { family in
                            Button {
                                withAnimation(.snappy(duration: 0.2)) {
                                    if shown.contains(family) { shown.remove(family) } else { shown.insert(family) }
                                }
                            } label: {
                                HStack {
                                    Image(systemName: Self.symbol(family))
                                    Text(family)
                                    Spacer()
                                    Image(systemName: "chevron.down").rotationEffect(.degrees(shown.contains(family) ? 180 : 0))
                                }
                                .padding(10)
                                .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                            }
                            .buttonStyle(.plain)
                            .padding(.top, 4)
                            if shown.contains(family) { subsections(family) }
                        }
                    }
                    if buttoned.isEmpty {
                        ForEach(rest, id: \.title) { section in
                            Text(section.title).font(.subheadline.weight(.semibold)).padding(.top, 10)
                            controls(section, skipping: [])
                            if section.title == "AI Denoise" { AiDenoiseStatus(model: model) }
                        }
                    }
                }
                .padding(.horizontal, 16).padding(.bottom, 14)
            }
            Rectangle().fill(Theme.line).frame(height: 0.5)
        }
    }

    @ViewBuilder private func controls(_ section: Section, skipping: [Switch]) -> some View {
        ForEach(section.switches.filter { !skipping.contains($0.id) }, id: \.label) { item in
            SwitchRow(item: item, model: model)
        }
        ForEach(section.sliders, id: \.label) { slider in
            SliderRow(slider: slider, model: model, track: SliderRow.track(for: slider.label, in: section.title))
        }
        if section.title == "AI Denoise", name == main?.title { AiDenoiseStatus(model: model) }
    }

    /// A family's sections, each under its view's name ("Hue", "Shadows").
    @ViewBuilder private func subsections(_ family: String) -> some View {
        ForEach(rest.filter { Self.family($0.title) == family }, id: \.title) { section in
            let view = section.title.components(separatedBy: " · ").dropFirst().first
            if let view { Text(view).font(.subheadline.weight(.semibold)).padding(.top, 8) }
            if section.title == "Tone Curve" { PointCurveEditor(model: model) }
            controls(section, skipping: [])
        }
    }

    private func toggleButton(_ family: String, label: Image) -> some View {
        Button {
            if !isOpen { isOpen = true }
            withAnimation(.snappy(duration: 0.2)) {
                if shown.contains(family) { shown.remove(family) } else { shown.insert(family) }
            }
        } label: {
            label.font(.callout)
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(shown.contains(family) ? Theme.accent : Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(family)
    }

    private static func symbol(_ family: String) -> String {
        switch family {
        case "Color Mixer", "B&W Mix": "paintpalette"
        case "Color Grading": "circle.hexagongrid"
        default: "slider.vertical.3"
        }
    }
}

/// White balance: its menu (As Shot, Auto, the presets, Custom) and the
/// eyedropper.
struct WhiteBalanceRow: View {
    @Bindable var model: LiveModel

    var body: some View {
        if let edit = model.edit, !edit.whiteBalanceChoices.isEmpty {
            HStack {
                Text("WB").foregroundStyle(Theme.secondaryText)
                Menu {
                    ForEach(edit.whiteBalanceChoices, id: \.self) { choice in
                        Button(choice) {
                            if choice == "Auto" {
                                model.runAuto(.whiteBalance)
                            } else {
                                model.live?.setWhiteBalance(name: choice)
                                model.refresh()
                            }
                        }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text(model.autoRunning == .whiteBalance ? "Auto…" : edit.whiteBalance)
                        Image(systemName: "chevron.down").font(.caption)
                    }
                }
                Spacer()
                Button {
                    model.tool = model.tool == .whiteBalance ? .none : .whiteBalance
                } label: {
                    Image(systemName: "eyedropper").padding(8)
                        .background(model.tool == .whiteBalance ? Theme.accent : Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
            }
            if model.tool == .whiteBalance {
                Text("Tap something that should be neutral grey or white.")
                    .font(.caption).foregroundStyle(Theme.secondaryText)
            }
        }
    }
}

/// Crop and geometry: the aspect ratios as a grid of shapes, Straighten,
/// rotate and flip, and Transform; the photo shows the crop's handles while
/// this pane is open.
struct CropPane: View {
    let model: LiveModel
    @State private var showGeometry = false

    var body: some View {
        let geometry = (model.edit?.sections ?? []).filter { $0.group == "Geometry" }
        VStack(alignment: .leading, spacing: 0) {
            Text("Crop and Geometry").font(.title3.weight(.semibold)).padding(16)
            rule
            VStack(alignment: .leading, spacing: 14) {
                Text("Aspect").font(.headline)
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: 4), spacing: 10) {
                    ForEach(CropAspect.presets, id: \.0) { name, aspect in
                        Button { model.chooseAspect(aspect) } label: {
                            VStack(spacing: 6) {
                                AspectShape(aspect: aspect)
                                    .frame(height: 30)
                                Text(name).font(.caption2).lineLimit(1)
                            }
                            .frame(maxWidth: .infinity).padding(.vertical, 8)
                            .background(model.cropAspect == aspect ? Theme.chosen : .clear, in: RoundedRectangle(cornerRadius: 8))
                            .foregroundStyle(model.cropAspect == aspect ? Color.primary : Theme.secondaryText)
                        }
                        .buttonStyle(.plain)
                    }
                    Button { model.swapCropOrientation() } label: {
                        VStack(spacing: 6) {
                            Image(systemName: "rectangle.portrait.rotate").font(.title3).frame(height: 30)
                            Text("Swap").font(.caption2)
                        }
                        .frame(maxWidth: .infinity).padding(.vertical, 8)
                    }
                    .buttonStyle(.plain)
                    .disabled(model.cropAspect == .free)
                }
                HStack(spacing: 0) {
                    tool("Rotate Left", "rotate.left") { model.orient(.rotateLeft) }
                    tool("Rotate Right", "rotate.right") { model.orient(.rotateRight) }
                    tool("Flip H", "arrow.left.and.right.righttriangle.left.righttriangle.right") { model.orient(.flipHorizontal) }
                    tool("Flip V", "arrow.up.and.down.righttriangle.up.righttriangle.down") { model.orient(.flipVertical) }
                }
                ForEach(geometry.first?.sliders ?? [], id: \.label) { SliderRow(slider: $0, model: model) }
            }
            .padding(16)
            rule
            Button {
                withAnimation(.snappy(duration: 0.2)) { showGeometry.toggle() }
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: "chevron.right").font(.footnote.weight(.semibold))
                        .rotationEffect(.degrees(showGeometry ? 90 : 0))
                    Text("Geometry").font(.headline)
                    Spacer()
                }
                .padding(16).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if showGeometry {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(geometry.dropFirst(), id: \.title) { section in
                        ForEach(section.switches, id: \.label) { SwitchRow(item: $0, model: model) }
                        ForEach(section.sliders, id: \.label) { SliderRow(slider: $0, model: model) }
                    }
                }
                .padding(.horizontal, 16).padding(.bottom, 14)
            }
            rule
            Button("Reset Crop", systemImage: "arrow.counterclockwise") { model.setCrop(CropRect(l: 0, t: 0, r: 1, b: 1), done: true) }
                .padding(16)
        }
    }

    private var rule: some View { Rectangle().fill(Theme.line).frame(height: 0.5) }

    private func tool(_ name: String, _ symbol: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: 6) {
                Image(systemName: symbol).font(.title3).frame(height: 26)
                Text(name).font(.caption2).foregroundStyle(Theme.secondaryText)
            }
            .frame(maxWidth: .infinity)
        }
        .buttonStyle(.plain)
    }
}

/// A ratio drawn as its shape: a rectangle that wide for that high; Free a
/// dashed one, Original the photo's frame.
struct AspectShape: View {
    let aspect: CropAspect

    var body: some View {
        switch aspect {
        case .free:
            RoundedRectangle(cornerRadius: 2).stroke(style: StrokeStyle(lineWidth: 1.5, dash: [3, 2]))
                .frame(width: 30, height: 22)
        case .original:
            Image(systemName: "photo").font(.title3)
        case .ratio(let r):
            let long: CGFloat = 30
            RoundedRectangle(cornerRadius: 2).stroke(lineWidth: 1.5)
                .frame(width: long, height: long / CGFloat(r))
        }
    }
}

/// A slider as the panel shows it: a thin track and a small knob. A drag
/// moves the value by how far the finger goes, wherever it starts, so a
/// touch never jumps it; one drag is one History step.
struct ThinSlider: View {
    let value: Float
    let range: ClosedRange<Float>
    /// Colours along the track, as Temp's blue to yellow; plain without.
    var track: [Color] = []
    let changed: (Float, Bool) -> Void
    @State private var start: Float?

    private static let knob: CGFloat = 16

    var body: some View {
        GeometryReader { geometry in
            let span = range.upperBound - range.lowerBound
            let travel = max(geometry.size.width - Self.knob, 1)
            let at = span > 0 ? CGFloat((value - range.lowerBound) / span) : 0
            ZStack(alignment: .leading) {
                if track.isEmpty {
                    Capsule().fill(Theme.line).frame(height: 2)
                } else {
                    Capsule().fill(LinearGradient(colors: track, startPoint: .leading, endPoint: .trailing)).frame(height: 3)
                }
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
