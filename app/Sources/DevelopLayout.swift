import SwiftUI
import UniformTypeIdentifiers

/// What the editor's right-hand panel shows, chosen on the tool rail.
enum DevelopPane: String, CaseIterable {
    case edit, crop, masks, heal, presets, history

    var title: String {
        switch self {
        case .edit: "Edit"
        case .crop: "Crop"
        case .masks: "Masks"
        case .heal: "Heal"
        case .presets: "Presets"
        case .history: "History"
        }
    }

    /// A tool with Cancel and Apply, rather than a panel of settings.
    var isTool: Bool { self == .crop || self == .masks || self == .heal }

    var symbol: String {
        switch self {
        case .edit: "slider.horizontal.3"
        case .crop: "crop.rotate"
        case .masks: "circle.dashed.inset.filled"
        case .heal: "bandage"
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
    /// Along the bottom (an iPhone) rather than down the right edge.
    var across = false
    @State private var ratingOpen = false

    var body: some View {
        let stack = across ? AnyLayout(HStackLayout(spacing: 4)) : AnyLayout(VStackLayout(spacing: 10))
        stack {
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
        .padding(across ? .horizontal : .vertical, across ? 8 : 12)
        .frame(width: across ? nil : 58, height: across ? 54 : nil)
        .frame(maxWidth: across ? .infinity : nil, maxHeight: across ? nil : .infinity)
        .background(Theme.panel)
    }
}

/// The right-hand panel: the chosen pane's tools. On an iPhone it fills
/// the width under the photo.
struct DevelopPanel: View {
    let model: LiveModel
    var fullWidth = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    switch model.pane {
                    case .edit: EditPane(model: model)
                    case .crop: CropPane(model: model)
                    case .masks: MasksPanel(model: model).padding(16)
                    case .heal: HealPane(model: model).padding(16)
                    case .presets: PresetsPanel(model: model).padding(16)
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
            if model.pane.isTool {
                Rectangle().fill(Theme.line).frame(height: 0.5)
                HStack(spacing: 18) {
                    Spacer()
                    Button("Cancel") { model.cancelTool() }
                    Button("Apply") { model.applyTool() }
                        .buttonStyle(.borderedProminent)
                }
                .padding(14)
            }
        }
        .frame(width: fullWidth ? nil : 340)
        .frame(maxWidth: fullWidth ? .infinity : nil, maxHeight: .infinity)
        .background(Theme.raised)
    }
}

/// Edit, as Lightroom for iPad lays it out: Auto, the profile, then the
/// groups Light, Color, Effects, Detail and Optics, each folding.
struct EditPane: View {
    let model: LiveModel
    // -color-mix and -grading open those panels, to look at.
    @State private var open: Set<String> = ProcessInfo.processInfo.arguments.contains("-color-mix") ? ["Color"] : ["Light"]
    /// Color Grading, opened over the groups as its own pane.
    @State private var grading = ProcessInfo.processInfo.arguments.contains("-grading")

    static let groups = ["Light", "Color", "Effects", "Detail", "Optics"]

    var body: some View {
        if grading {
            ColorGradingPane(model: model) { grading = false }
        } else {
            groups
        }
    }

    private var groups: some View {
        let sections = model.edit?.sections ?? []
        return VStack(alignment: .leading, spacing: 0) {
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
                    openGrading: { grading = true },
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
    @State private var importing = false
    @State private var imported: ImportSummary?
    @State private var working = false
    /// Fit Camera Look's progress, or why it fitted nothing.
    @State private var fitNote: String?

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
                    Divider()
                    Button("Fit Camera Look", systemImage: "camera.filters") { fitCameraLook() }
                    Button("Import Profiles…", systemImage: "square.and.arrow.down") { importing = true }
                } label: {
                    Group {
                        if working { ProgressView().controlSize(.small) } else { Text("Browse") }
                    }
                    .padding(.horizontal, 12).padding(.vertical, 7)
                    .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
                .disabled(working)
            }
            if let fitNote {
                Text(fitNote).font(.caption).foregroundStyle(Theme.secondaryText)
            }
            ForEach(amount?.sliders ?? [], id: \.label) { SliderRow(slider: $0, model: model) }
        }
        .padding(16)
        .fileImporter(
            isPresented: $importing,
            allowedContentTypes: ["dcp", "xmp", "cube"].map { UTType(filenameExtension: $0) ?? .data } + [.folder],
            allowsMultipleSelection: true
        ) { result in
            guard case .success(let picks) = result, !picks.isEmpty else { return }
            working = true
            let live = model.live
            Task {
                imported = await Task.detached { Self.runImport(picks, live: live) }.value
                model.refresh()
                working = false
            }
        }
        .alert(imported?.message ?? "", isPresented: Binding(get: { imported != nil }, set: { if !$0 { imported = nil } })) {
            Button("OK") { imported = nil }
        } message: {
            Text((imported?.details.prefix(6) ?? []).joined(separator: "\n"))
        }
    }

    /// Profile → Fit Camera Look, as on the desktop: a look fitted to the
    /// RAW's embedded JPEG, used as its profile; takes a few seconds.
    private func fitCameraLook() {
        working = true
        fitNote = "Fitting the camera's look…"
        let live = model.live
        Task {
            let result = await Task.detached { Result { try live?.fitCameraLook() } }.value
            switch result {
            case .success: fitNote = nil
            case .failure(RenderError.Failed(let message)): fitNote = message
            case .failure(let error): fitNote = "\(error)"
            }
            model.refresh()
            working = false
        }
    }

    /// Imports the profiles, looks and LUTs in `picks` (files or folders)
    /// while their security scope is open, then lists the photo's profiles
    /// again.
    nonisolated private static func runImport(_ picks: [URL], live: LiveView?) -> ImportSummary {
        let opened = picks.filter { $0.startAccessingSecurityScopedResource() }
        defer { opened.forEach { $0.stopAccessingSecurityScopedResource() } }
        let result = importProfiles(paths: picks.map(\.path))
        if result.done > 0 { live?.reloadProfiles() }
        return result
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
    var openGrading: () -> Void = {}
    @Binding var isOpen: Bool
    /// The families opened from buttons ("Tone Curve", "Color Mixer"…).
    @State private var shown: Set<String> = {
        let args = ProcessInfo.processInfo.arguments
        return Set((args.contains("-color-mix") ? ["Color Mixer"] : []) + (args.contains("-tone-curve") ? ["Tone Curve"] : []))
    }()

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
                    if name == "Optics" { LensProfileRow(model: model); LensfunCard(model: model) }
                    if name == "Light", shown.contains("Tone Curve") {
                        subsections("Tone Curve")
                    }
                    if name == "Color" {
                        ForEach(buttoned, id: \.self) { family in
                            Button {
                                if family == "Color Grading" { openGrading(); return }
                                withAnimation(.snappy(duration: 0.2)) {
                                    if shown.contains(family) { shown.remove(family) } else { shown.insert(family) }
                                }
                            } label: {
                                HStack {
                                    Image(systemName: Self.symbol(family))
                                    Text(family)
                                    Spacer()
                                    Image(systemName: family == "Color Grading" ? "chevron.right" : "chevron.down")
                                        .rotationEffect(.degrees(shown.contains(family) ? 180 : 0))
                                }
                                .padding(10)
                                .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                            }
                            .buttonStyle(.plain)
                            .padding(.top, 4)
                            if shown.contains(family) {
                                if family == "Color Mixer" {
                                    ColorMixView(sections: rest.filter { Self.family($0.title) == family }, model: model)
                                        .padding(.top, 6)
                                } else {
                                    subsections(family)
                                }
                            }
                        }
                    }
                    if buttoned.isEmpty {
                        ForEach(rest, id: \.title) { section in
                            Text(section.title).font(.subheadline.weight(.semibold)).padding(.top, 10)
                            controls(section, skipping: [])
                        }
                    }
                }
                .padding(.horizontal, 16).padding(.bottom, 14)
            }
            Rectangle().fill(Theme.line).frame(height: 0.5)
        }
    }

    @ViewBuilder private func controls(_ section: Section, skipping: [Switch]) -> some View {
        if section.title == "Calibration" { CalibrationProcess(model: model) }
        ForEach(section.switches.filter { !skipping.contains($0.id) }, id: \.label) { item in
            SwitchRow(item: item, model: model)
            if item.id == .hdr, item.on, let note = model.live?.hdrNote() {
                Text(note).font(.caption).foregroundStyle(Theme.secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        ForEach(section.sliders, id: \.label) { slider in
            SliderRow(slider: slider, model: model, track: SliderRow.track(for: slider.label, in: section.title))
        }
        if section.title == "AI Denoise" { AiDenoiseStatus(model: model) }
    }

    /// A family's sections, each under its view's name ("Hue", "Shadows"),
    /// the Tone Curve and the B&W mix with their targeted adjustment.
    @ViewBuilder private func subsections(_ family: String) -> some View {
        if family == "Tone Curve" || family == "B&W Mix" {
            HStack {
                TargetedHint(model: model)
                Spacer()
                TargetButton(model: model, kind: family == "Tone Curve" ? .toneCurve : .blackWhite)
            }
            .padding(.top, 6)
        }
        ForEach(rest.filter { Self.family($0.title) == family }, id: \.title) { section in
            let view = section.title.components(separatedBy: " · ").dropFirst().first
            if let view { Text(view).font(.subheadline.weight(.semibold)).padding(.top, 8) }
            if section.title == "Tone Curve" {
                ToneCurveView(model: model, section: section)
            } else {
                controls(section, skipping: [])
            }
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
                Text("Move the circle onto something neutral grey or white, then tap ✓.")
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
    /// A ratio of the user's own, width by height, and its entry.
    @State private var custom: (Float, Float)?
    @State private var askingRatio = false
    @State private var ratioWidth = ""
    @State private var ratioHeight = ""

    private static func number(_ v: Float) -> String {
        v.rounded() == v ? String(Int(v)) : String(format: "%g", v)
    }

    /// Auto, and the ruler drawn along something that should be level.
    private var straighten: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Button {
                    model.straightenNote = "Measuring…"
                    let live = model.live
                    Task {
                        let result = await Task.detached { Result { try live?.autoStraighten() } }.value
                        switch result {
                        case .failure(RenderError.Failed(let message)): model.straightenNote = message
                        case .failure(let error): model.straightenNote = "\(error)"
                        case .success: model.straightenNote = nil
                        }
                        model.refresh()
                    }
                } label: {
                    Label("Auto", systemImage: "wand.and.stars").padding(.horizontal, 10).padding(.vertical, 6)
                        .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                .disabled(model.straightenNote == "Measuring…")
                Button {
                    model.cropRuler.toggle()
                } label: {
                    Label("Ruler", systemImage: "ruler").padding(.horizontal, 10).padding(.vertical, 6)
                        .background(model.cropRuler ? Theme.accent : Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
            }
            if model.cropRuler {
                Text("Draw along something that should be level or upright.")
                    .font(.caption).foregroundStyle(Theme.secondaryText)
            } else if let note = model.straightenNote {
                Text(note).font(.caption).foregroundStyle(Theme.secondaryText)
            }
        }
    }

    /// The guide overlay over the crop, and a turn for those that turn.
    private var guides: some View {
        let all = cropGuides()
        let chosen = all.first { $0.index == model.cropGuide }
        return HStack {
            Text("Overlay").foregroundStyle(Theme.secondaryText)
            Spacer()
            Menu {
                Button { model.cropGuide = nil } label: {
                    if model.cropGuide == nil { Label("None", systemImage: "checkmark") } else { Text("None") }
                }
                ForEach(all, id: \.index) { guide in
                    Button {
                        model.cropGuide = guide.index
                        model.cropGuideOrientation = 0
                    } label: {
                        if guide.index == model.cropGuide { Label(guide.name, systemImage: "checkmark") } else { Text(guide.name) }
                    }
                }
            } label: {
                HStack(spacing: 4) {
                    Text(chosen?.name ?? "None")
                    Image(systemName: "chevron.down").font(.caption)
                }
            }
            if let chosen, chosen.orientations > 1 {
                Button {
                    model.cropGuideOrientation = (model.cropGuideOrientation + 1) % chosen.orientations
                } label: {
                    Image(systemName: "arrow.triangle.2.circlepath").padding(6)
                        .background(Theme.chosen, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Turn the overlay")
            }
        }
    }

    var body: some View {
        let geometry = (model.edit?.sections ?? []).filter { $0.group == "Geometry" }
        VStack(alignment: .leading, spacing: 0) {
            Text("Crop and Geometry").font(.title3.weight(.semibold)).padding(16)
                .alert("Custom Aspect Ratio", isPresented: $askingRatio) {
                    TextField("Width", text: $ratioWidth).keyboardType(.decimalPad)
                    TextField("Height", text: $ratioHeight).keyboardType(.decimalPad)
                    Button("Use") {
                        let parse = { (t: String) in Float(t.replacingOccurrences(of: ",", with: ".")) }
                        if let w = parse(ratioWidth), let h = parse(ratioHeight), (0.01...1000).contains(w), (0.01...1000).contains(h) {
                            custom = (w, h)
                            model.chooseAspect(.ratio(w / h))
                        }
                    }
                    Button("Cancel", role: .cancel) {}
                } message: {
                    Text("Width and height, as 5 and 7 for 5 × 7.")
                }
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
                    Button { askingRatio = true } label: {
                        VStack(spacing: 6) {
                            if let custom {
                                AspectShape(aspect: .ratio(custom.0 / custom.1)).frame(height: 30)
                                Text("\(Self.number(custom.0)) × \(Self.number(custom.1))").font(.caption2).lineLimit(1)
                            } else {
                                Image(systemName: "plus.rectangle").font(.title3).frame(height: 30)
                                Text("Custom").font(.caption2)
                            }
                        }
                        .frame(maxWidth: .infinity).padding(.vertical, 8)
                        .background(custom.map { model.cropAspect == .ratio($0.0 / $0.1) } == true ? Theme.chosen : .clear,
                                    in: RoundedRectangle(cornerRadius: 8))
                    }
                    .buttonStyle(.plain)
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
                straighten
                guides
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
                    UprightRow(model: model)
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

/// Lens Corrections' profile: Auto (the imported profile that fits the
/// lens best), or one of the imported Adobe profiles for this camera, those
/// for this lens first.
struct LensProfileRow: View {
    let model: LiveModel

    var body: some View {
        let _ = model.edit?.applied
        let profiles = model.live?.lensProfiles() ?? []
        if !profiles.isEmpty {
            HStack {
                Text("Lens Profile").foregroundStyle(Theme.secondaryText)
                Spacer()
                Menu {
                    Button("Auto") { model.live?.setLensProfile(index: nil); model.refresh() }
                    Divider()
                    ForEach(profiles, id: \.index) { p in
                        Button {
                            model.live?.setLensProfile(index: p.index)
                            model.refresh()
                        } label: {
                            if p.used { Label(p.name, systemImage: "checkmark") } else { Text(p.name) }
                        }
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text(profiles.first(where: \.used)?.name ?? "None").lineLimit(1)
                        Image(systemName: "chevron.down").font(.caption)
                    }
                }
            }
            .font(.callout)
        }
    }
}

/// Transform's Upright, as the desktop's: Off, Auto, Level, Vertical and
/// Full, each analysing the photo the first time it needs it, and Update to
/// analyse again (after changing lens corrections).
struct UprightRow: View {
    let model: LiveModel
    @State private var working = false
    @State private var note: String?

    private static let modes: [(Upright, String)] = [
        (.off, "Off"), (.auto, "Auto"), (.level, "Level"), (.vertical, "Vertical"), (.full, "Full"),
        (.guided, "Guided"),
    ]

    var body: some View {
        let _ = model.edit?.applied
        let current = model.live?.upright()
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Upright").foregroundStyle(Theme.secondaryText)
                Spacer()
                if working { ProgressView().controlSize(.small) }
                Button("Update") { choose(current ?? .off, again: true) }
                    .font(.callout)
                    .disabled(working || current == nil || current == .off)
            }
            HStack(spacing: 4) {
                ForEach(Self.modes, id: \.1) { mode, name in
                    Button {
                        // Guided opens its guides on the photo, as the desktop's Guided tool.
                        if mode == .guided { model.guiding = true }
                        choose(mode, again: false)
                    } label: {
                        Text(name).font(.caption).lineLimit(1)
                            .frame(maxWidth: .infinity).padding(.vertical, 6)
                            .background(current == mode ? Theme.chosen : .clear, in: RoundedRectangle(cornerRadius: 6))
                            .foregroundStyle(current == mode ? Color.primary : Theme.secondaryText)
                    }
                    .buttonStyle(.plain)
                    .disabled(working)
                }
            }
            if let note {
                Text(note).font(.caption).foregroundStyle(Theme.secondaryText)
            }
        }
        .padding(.bottom, 6)
    }

    private func choose(_ mode: Upright, again: Bool) {
        working = true
        note = nil
        let live = model.live
        Task {
            let result = await Task.detached { Result { try live?.setUpright(mode: mode, again: again) } }.value
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
/// touch never jumps it; one drag is one History step. Two touches that
/// don't move, close together, are a double tap and call `reset`: told
/// apart here, by the one gesture that takes every touch on the track,
/// rather than by a second recognizer that touch would have to win.
struct ThinSlider: View {
    let value: Float
    let range: ClosedRange<Float>
    /// Colours along the track, as Temp's blue to yellow; plain without.
    var track: [Color] = []
    var reset: (() -> Void)?
    let changed: (Float, Bool) -> Void
    @State private var start: Float?
    /// When a touch that didn't move last ended, for the double tap.
    @State private var lastTap: Date?

    private static let knob: CGFloat = 16
    /// How far a touch may move and still be a tap, in points.
    private static let tapSlop: CGFloat = 6
    /// How soon the second tap must follow the first.
    private static let doubleTap: TimeInterval = 0.45

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
                        let tapped = abs(drag.translation.width) < Self.tapSlop
                            && abs(drag.translation.height) < Self.tapSlop
                        if tapped, let last = lastTap, Date().timeIntervalSince(last) < Self.doubleTap, let reset {
                            lastTap = nil
                            start = nil
                            reset()
                            return
                        }
                        lastTap = tapped ? Date() : nil
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

/// Calibration's process version, as the desktop shows it (RAWmakase renders
/// every edit with the current one), and its button for the camera's
/// exposure baseline when the edit doesn't use it.
struct CalibrationProcess: View {
    let model: LiveModel

    var body: some View {
        let _ = model.edit?.applied
        HStack {
            Text("Process").foregroundStyle(Theme.secondaryText)
            Spacer()
            Text("Current version").foregroundStyle(Theme.secondaryText)
        }
        .font(.callout)
        if model.live?.offersCameraExposureBaseline() == true {
            Button("Use Camera Exposure Baseline") {
                model.live?.useCameraExposureBaseline()
                model.refresh()
            }
            .font(.callout)
        }
    }
}

/// Lensfun's community lens profiles where no imported Adobe profile fits,
/// as the desktop's Lensfun card offers them: the download (on request, with
/// its terms), then the lens this photo matches, a choice when the name fits
/// several or none, and resolving or updating the lens an edit names.
struct LensfunCard: View {
    let model: LiveModel
    @State private var working = false
    @State private var said: String?

    var body: some View {
        let _ = model.edit?.applied
        let _ = working
        let offer = model.live?.lensfun() ?? LensfunOffer(title: "", note: "", action: .none, lenses: [], chosen: nil, installed: nil, terms: "")
        if !offer.title.isEmpty || offer.chosen != nil {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("Lensfun").foregroundStyle(Theme.secondaryText)
                    Spacer()
                    if let chosen = offer.chosen { Text(chosen).lineLimit(1) }
                    if working { ProgressView().controlSize(.small) }
                    if offer.installed != nil {
                        Menu {
                            Button("Check for Newer Profiles", systemImage: "arrow.down.circle") { install() }
                            Button("Remove Lensfun Profiles", systemImage: "trash", role: .destructive) { remove() }
                        } label: { Image(systemName: "ellipsis.circle") }
                            .disabled(working)
                    }
                }
                if !offer.title.isEmpty {
                    Text(offer.title).font(.callout.weight(.semibold))
                    Text(offer.note).font(.caption).foregroundStyle(Theme.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }
                action(offer)
                if !offer.terms.isEmpty {
                    Text(offer.terms).font(.caption2).foregroundStyle(Theme.secondaryText)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if let said {
                    Text(said).font(.caption).foregroundStyle(Theme.secondaryText)
                }
            }
            .font(.callout)
            .padding(10)
            .background(Theme.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    @ViewBuilder private func action(_ offer: LensfunOffer) -> some View {
        switch offer.action {
        case .none:
            EmptyView()
        case .download:
            Button("Download · 0.5 MB") { install() }.disabled(working)
        case .use(let index):
            Button("Use") { model.live?.useLensfun(index: index); model.refresh() }
        case .choose:
            Menu("Choose a Lens…") {
                ForEach(offer.lenses, id: \.index) { lens in
                    Button(lens.label) { model.live?.useLensfun(index: lens.index); model.refresh() }
                }
            }
        case .resolve:
            Button("Resolve") { model.live?.useLensfun(index: nil); model.refresh() }
        case .update:
            Button("Update") { model.live?.useLensfun(index: nil); model.refresh() }
        }
    }

    private func install() {
        working = true
        said = nil
        Task {
            let result = await Task.detached { Result { try installLensfun() } }.value
            switch result {
            case .success(let text): said = text
            case .failure(RenderError.Failed(let message)): said = "The download failed: \(message)"
            case .failure(let error): said = "The download failed: \(error)"
            }
            working = false
            model.refresh()
        }
    }

    private func remove() {
        working = true
        Task {
            let result = await Task.detached { Result { try removeLensfun() } }.value
            if case .failure(let error) = result { said = "\(error)" } else { said = "Removed Lensfun's profiles. Edits keep their lens's calibration." }
            working = false
            model.refresh()
        }
    }
}
