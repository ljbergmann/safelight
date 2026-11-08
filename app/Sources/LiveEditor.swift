import SwiftUI

/// The live view and what it should show. A class, so the layer view's
/// callback and SwiftUI's change handlers see the same state, whichever of
/// the view and the photo comes first.
@MainActor @Observable
final class LiveModel {
    var live: LiveView? { didSet { open() } }
    var library: Library?
    var photo: LibraryPhoto? { didSet { if photo?.id != oldValue?.id { open() } } }
    var size: DecodeSize = .half { didSet { if size != oldValue { open() } } }
    /// The sliders, History and save state, as the core last reported them.
    private(set) var edit: EditState?
    /// A slider being dragged, and its value: shown instead of the core's
    /// until the drag ends.
    var dragging: (Control, Float)?
    /// The Crop tool is open; zoom and pan wait while it is.
    var cropping = false {
        didSet {
            live?.setCropping(on: cropping); zoom = .fit
            // Tools work on the edit alone, as on the desktop.
            if cropping && compare != .off { compare = .off }
        }
    }
    /// Before/After: Before alone, beside the edit, or split across it.
    var compare: Compare = .off { didSet { live?.setCompare(compare: compare) } }
    var cropAspect: CropAspect = .free
    /// The chosen ratio turned to portrait (the crop bar's Swap).
    var cropPortrait = false
    /// Settings were copied in this editor (the catalog may also hold some).
    var copied = false
    var copying = false
    var exporting = false
    /// What a touch on the photo does (masks, Heal); zoom and pan wait while
    /// a tool is open.
    var tool: PhotoTool = .none { didSet { if tool != .none && compare != .off { compare = .off } } }
    var selectedMask: UInt32?
    var brushSize: Float = 0.04
    var brushFeather: Float = 0.5
    var brushFlow: Float = 1
    var brushErase = false
    var spotSize: Float = 0.02
    var cloneSpots = false
    /// A Select Subject, Sky or Background run in progress, and why the last
    /// one failed.
    var selecting: AiSelection?
    /// An Auto estimate running.
    var autoRunning: AutoKind?
    /// Show the selected mask in red over the photo, as Lightroom's overlay.
    var showMaskOverlay = true
    var selectionError: String?
    /// The model whose download the install sheet offers.
    var offeredModel: AiModel?
    /// What a paste couldn't carry over, shown below the controls.
    var pasteNote: String?
    /// Fit, or 100% around a centre in fractions of the developed photo.
    var zoom: Zoom = .fit { didSet { live?.setZoom(zoom: zoom) } }
    /// A pinch in progress, shown by scaling the last frame until it ends.
    var pinch: CGFloat = 1
    var pinchAnchor: UnitPoint = .center
    /// The centre a drag started from.
    var dragStart: (Float, Float)?

    private func open() {
        guard let live, let photo, let library else { return }
        // What belonged to the last photo.
        edit = nil
        dragging = nil
        cropping = false
        tool = .none
        selectedMask = nil
        pasteNote = nil
        zoom = .fit
        live.setCompare(compare: compare)
        live.openPhoto(library: library, id: photo.id, path: photo.path, decode: size)
    }

    /// Reads the edit back from the core: none while the core still holds
    /// the last photo's.
    func refresh() {
        let state = live?.editState()
        edit = state?.photo == photo?.id ? state : nil
    }

    /// Called with -1 or +1 when a swipe at Fit asks for the previous or
    /// next photo.
    var onSwipe: ((Int) -> Void)?

    func set(_ control: Control, to value: Float, done: Bool) {
        dragging = done ? nil : (control, value)
        live?.setControl(control: control, value: value, done: done)
        refresh()
    }

    func setSwitch(_ which: Switch, on: Bool) {
        live?.setSwitch(which: which, on: on)
        refresh()
    }

    func undo() { live?.undo(); refresh() }
    func redo() { live?.redo(); refresh() }

    /// Saves the edit; the error, when it couldn't be, stays in `edit`.
    @discardableResult
    func save() -> Bool {
        guard let live else { return true }
        let saved: Bool
        do {
            try live.save()
            saved = true
        } catch {
            saved = false
        }
        refresh()
        print("SAVE \(saved ? "ok" : "failed: \(edit?.saveError ?? "?")"); unsaved: \(edit?.needsSave ?? false)")
        return saved
    }

    /// The point of the developed photo under `location` (in the view's
    /// points), as fractions of its size, for a view `size` points large.
    func photoPoint(at location: CGPoint, in size: CGSize, scale: CGFloat) -> (Float, Float)? {
        guard let stats = live?.stats(), stats.fullWidth > 0 else { return nil }
        let (fw, fh) = (CGFloat(stats.fullWidth), CGFloat(stats.fullHeight))
        // Within the edit's side of a Before/After view, as the core lays it out.
        let area = afterArea(compare: compare, width: Float(size.width * scale), height: Float(size.height * scale)).map { CGFloat($0) }
        let (w, h) = (area[2], area[3])
        let (x, y) = (location.x * scale - area[0], location.y * scale - area[1])
        switch zoom {
        case .fit:
            let s = min(w / fw, h / fh)
            let (left, top) = ((w - fw * s) / 2, (h - fh * s) / 2)
            return (Float(min(max((x - left) / (fw * s), 0), 1)), Float(min(max((y - top) / (fh * s), 0), 1)))
        case .actual(let cx, let cy):
            return (cx + Float((x - w / 2) / fw), cy + Float((y - h / 2) / fh))
        }
    }

    /// Fit ↔ 100% at `location`.
    func toggleZoom(at location: CGPoint, in size: CGSize, scale: CGFloat) {
        guard !cropping && tool == .none else { return }
        if case .fit = zoom, let (x, y) = photoPoint(at: location, in: size, scale: scale) {
            zoom = .actual(centerX: x, centerY: y)
        } else {
            zoom = .fit
        }
    }

    /// Ends a pinch: in from Fit to 100% around its anchor, or out to Fit.
    func endPinch(in size: CGSize, scale: CGFloat) {
        guard !cropping && tool == .none else { pinch = 1; return }
        let anchor = CGPoint(x: pinchAnchor.x * size.width, y: pinchAnchor.y * size.height)
        switch zoom {
        case .fit where pinch > 1.25:
            if let (x, y) = photoPoint(at: anchor, in: size, scale: scale) {
                zoom = .actual(centerX: x, centerY: y)
            }
        case .actual where pinch < 0.8:
            zoom = .fit
        default:
            break
        }
        pinch = 1
    }

    /// A swipe across the photo at Fit, with no tool open: the previous or
    /// next photo, as Lightroom's loupe.
    func swipe(by translation: CGSize) {
        guard !cropping && tool == .none, case .fit = zoom, compare == .off || compare == .beforeOnly else { return }
        guard abs(translation.width) > 80, abs(translation.width) > 2 * abs(translation.height) else { return }
        onSwipe?(translation.width < 0 ? 1 : -1)
    }

    /// Pans at 100% by `translation` points from where the drag started.
    func drag(by translation: CGSize, scale: CGFloat) {
        guard !cropping && tool == .none else { return }
        guard case .actual(let cx, let cy) = zoom, let stats = live?.stats(), stats.fullWidth > 0 else { return }
        if dragStart == nil { dragStart = (cx, cy) }
        let (sx, sy) = dragStart!
        zoom = .actual(
            centerX: sx - Float(translation.width * scale / CGFloat(stats.fullWidth)),
            centerY: sy - Float(translation.height * scale / CGFloat(stats.fullHeight)))
    }

    /// Ends a drag on the centre the core kept inside the photo.
    func endDrag() {
        dragStart = nil
        if let kept = live?.stats().zoom, case .actual = kept { zoom = kept }
    }
}

/// Stage 3: the photo drawn live by the core, with an Exposure slider.
struct LiveEditor: View {
    let photo: LibraryPhoto
    let library: Library
    let model: LiveModel
    @Environment(\.displayScale) private var displayScale

    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        let layout = sizeClass == .regular
            ? AnyLayout(HStackLayout(alignment: .top, spacing: 12))
            : AnyLayout(VStackLayout(spacing: 12))
        layout {
            VStack(spacing: 12) {
            GeometryReader { geometry in
                LiveViewRepresentable { [model] made in
                    model.live = made
                    if ProcessInfo.processInfo.arguments.contains("-sweep") { sweep(made) }
                    if ProcessInfo.processInfo.arguments.contains("-pan") { pan(made) }
                }
                .scaleEffect(model.pinch, anchor: model.pinchAnchor)
                .gesture(
                    MagnifyGesture()
                        .onChanged { value in
                            model.pinchAnchor = value.startAnchor
                            model.pinch = value.magnification
                        }
                        .onEnded { _ in model.endPinch(in: geometry.size, scale: displayScale) }
                )
                .simultaneousGesture(
                    DragGesture(minimumDistance: 4)
                        .onChanged { value in model.drag(by: value.translation, scale: displayScale) }
                        .onEnded { value in
                            model.endDrag()
                            model.swipe(by: value.translation)
                        }
                )
                .onTapGesture(count: 2) { location in
                    model.toggleZoom(at: location, in: geometry.size, scale: displayScale)
                }
                .overlay {
                    if model.cropping {
                        CropOverlay(model: model, size: geometry.size)
                    } else if model.tool != .none {
                        PhotoToolOverlay(model: model, size: geometry.size)
                    } else if model.compare != .off {
                        CompareOverlay(model: model, size: geometry.size)
                    } else if model.selectedMask != nil, case .fit = model.zoom {
                        MaskOnPhoto(model: model, size: geometry.size)
                    }
                }
            }
            .background(Color(white: 0.11))
            .clipShape(RoundedRectangle(cornerRadius: 8))

            if model.cropping { CropBar(model: model) }

            HStack(spacing: 18) {
                Button("Crop", systemImage: "crop") { model.cropping.toggle() }
                    .foregroundStyle(model.cropping ? Color.accentColor : .primary)
                BeforeAfterMenu(model: model)
                Button("Undo", systemImage: "arrow.uturn.backward") { model.undo() }
                    .disabled(!(model.edit?.canUndo ?? false))
                Button("Redo", systemImage: "arrow.uturn.forward") { model.redo() }
                    .disabled(!(model.edit?.canRedo ?? false))
                Button("Copy Settings", systemImage: "doc.on.doc") { model.copying = true }
                Button("Paste Settings", systemImage: "doc.on.clipboard") { model.paste() }
                    .disabled(!(model.copied || (library.hasCopiedSettings())))
                Button("Export", systemImage: "square.and.arrow.up") {
                    // The export reads the edit from the catalog.
                    if model.save() { model.exporting = true }
                }
                Spacer()
                saveStatus
                Picker("Decode", selection: Bindable(model).size) {
                    Text("Half size").tag(DecodeSize.half)
                    Text("Full size").tag(DecodeSize.full)
                }
                .pickerStyle(.menu)
                .fixedSize()
            }
            .labelStyle(.iconOnly)
            .font(.title3)
            Text(historyText).font(.caption).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let note = model.pasteNote {
                Text(note).font(.caption).foregroundStyle(.orange).frame(maxWidth: .infinity, alignment: .leading)
            }

            TimelineView(.periodic(from: .now, by: 0.25)) { _ in
                // Picks up the edit once it has loaded.
                // AI Denoise's progress arrives from the core on its own.
                let _ = (model.edit == nil || model.edit?.aiDenoise != nil) ? model.refresh() : ()
                Text(summary(model.live?.stats() ?? .empty))
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            }
            DevelopPanels(model: model)
                .frame(width: sizeClass == .regular ? 360 : nil)
                .frame(maxHeight: sizeClass == .regular ? .infinity : 360)
        }
        .padding()
        .sheet(isPresented: Bindable(model).copying) { CopySettingsSheet(model: model) }
        .sheet(isPresented: Bindable(model).exporting) { ExportSheet(library: library, ids: [photo.id]) }
        .sheet(item: Bindable(model).offeredModel) { offered in
            ModelInstallSheet(offered: offered) { model.live?.redraw(); model.refresh() }
        }
        .task { await scripted() }
        .onAppear {
            // -full: decode at full size from the start, for -sweep.
            if ProcessInfo.processInfo.arguments.contains("-full") { model.size = .full }
            model.library = library
            model.photo = photo
        }
        .onChange(of: photo.id) { model.photo = photo }
    }

    /// Launch options for checking edits without touching the screen: once
    /// the edit has loaded, prints it (`EDIT …`); `-set-exposure <stops>`
    /// moves Exposure as one drag, and `-save` then saves as leaving does.
    private func scripted() async {
        for _ in 0..<300 where model.edit == nil {
            model.refresh()
            try? await Task.sleep(for: .milliseconds(100))
        }
        let args = ProcessInfo.processInfo.arguments
        // The edit as opened: the tests below go back to it when they're
        // done, as the app saves whatever is shown when it is ended.
        let opened = model.edit?.applied ?? 0
        if let i = args.firstIndex(of: "-set-exposure"), i + 1 < args.count, let stops = Float(args[i + 1]) {
            model.set(.param(index: 2), to: stops, done: true)
        }
        if args.contains("-mask-test"), let live = model.live {
            // A radial mask in the middle at +1 EV, a brush stroke, a Heal spot.
            if let radial = live.addMask(kind: .radial, from: [0.5, 0.5], to: [0.65, 0.6]) {
                let exposure = live.maskSliders(index: radial).firstIndex { $0.label == "Exposure" }
                if let exposure {
                    live.setControl(control: .mask(mask: radial, index: UInt32(exposure)), value: 1, done: true)
                }
            }
            if let brush = live.addMask(kind: .brush, from: [0, 0], to: [0, 0]) {
                live.brush(index: brush, stroke: Stroke(points: [0.2, 0.3, 0.3, 0.35, 0.4, 0.4], radius: 0.03, feather: 0.5, flow: 0.8, erase: false))
            }
            let healed = live.addSpot(at: [0.7, 0.7], radius: 0.02, clone: false)
            model.refresh()
            print("MASKS \(model.edit?.masks.map { "\($0.name)·\($0.kind)" } ?? []); spots \(model.edit?.spots ?? 0) (heal found a source: \(healed)); steps \(model.edit?.steps ?? [])")
        }
        if args.contains("-develop-test"), let live = model.live {
            // White balance, the eyedropper, Auto, a curve drag and History.
            live.setWhiteBalance(name: "Daylight")
            live.pickWhiteBalance(at: [0.5, 0.5])
            let start = Date()
            let auto = await Task.detached { Result { try live.auto(kind: .settings) } }.value
            let autoMs = Int(Date().timeIntervalSince(start) * 1000)
            let i = live.setCurvePoint(channel: 0, index: nil, x: 0.5, y: 0.5, done: false)
            _ = live.setCurvePoint(channel: 0, index: i, x: 0.5, y: 0.6, done: true)
            let steps = live.editState()?.steps ?? []
            live.goToStep(applied: 1)
            let wb = live.editState()?.whiteBalance ?? "?"
            print("DEVELOP steps \(steps); auto \(auto) in \(autoMs) ms; curve \(live.pointCurve(channel: 0)?.points ?? []); after going to step 1: WB \(wb), applied \(live.editState()?.applied ?? 0)")
            live.goToStep(applied: 0)
            let profiles = live.profiles()
            if let other = profiles.first(where: { !$0.selected && $0.index != nil && $0.hasAmount }) ?? profiles.first(where: { !$0.selected }) {
                live.setProfile(index: other.index)
                let amount = live.editState()?.sections.first { $0.title == "Basic" }?.sliders.first?.label ?? "?"
                print("PROFILES \(profiles.count) in \(Set(profiles.map(\.group)).count - 1) groups; chose \(other.title); first Basic slider \(amount); last step \(live.editState()?.steps.last ?? "")")
                live.undo()
            }
            // The histogram is the last frame's: wait for one.
            let frames = live.stats().frames
            for _ in 0..<50 where live.stats().frames == frames { try? await Task.sleep(for: .milliseconds(100)) }
            let h = live.histogram()
            if let color = live.addMask(kind: .colorRange, from: [0.3, 0.3], to: [0.3, 0.3]) {
                _ = live.addMaskComponent(mask: color, kind: .radial, operation: .intersect, from: [0.5, 0.5], to: [0.7, 0.7])
                live.setMaskInvert(mask: color, component: nil, invert: true)
            }
            _ = live.addMask(kind: .luminanceRange, from: [0.6, 0.6], to: [0.6, 0.6])
            if let radial = live.addMask(kind: .radial, from: [0.5, 0.5], to: [0.6, 0.6]) {
                let before = live.gradientHandles(mask: radial, component: 0)
                live.dragGradientHandle(mask: radial, component: 0, handle: 0, from: [0.5, 0.5], to: [0.45, 0.5], done: false)
                live.dragGradientHandle(mask: radial, component: 0, handle: 0, from: [0.45, 0.5], to: [0.4, 0.5], done: true)
                let after = live.gradientHandles(mask: radial, component: 0)
                let start = Date()
                let overlay = await Task.detached { live.maskOverlay(index: radial, maxEdge: 640) }.value
                let covered = overlay?.alpha.filter { $0 > 128 }.count ?? 0
                print("GRADIENT centre \(before.prefix(2)) -> \(after.prefix(2)); last step \(live.editState()?.steps.last ?? ""); overlay \(overlay?.width ?? 0)x\(overlay?.height ?? 0), \(covered) px covered, \(Int(Date().timeIntervalSince(start) * 1000)) ms")
            }
            let masks = live.editState()?.masks.map { "\($0.kind)[\($0.components.map { "\($0.operation) \($0.kind)" }.joined(separator: ", "))]\($0.invert ? " inverted" : "")" } ?? []
            print("MASKS2 \(masks); histogram \(h.green.reduce(0, +)) pixels (frames \(frames) then \(live.stats().frames)), clipped shadows \(h.shadowsClipped) highlights \(h.highlightsClipped); first mask sliders \(live.maskSliders(index: 0).prefix(3).map(\.label))")
        }
        if args.contains("-develop-test"), let live = model.live {
            await presetsAndSnapshotsTest(live)
            await beforeAfterTest(live)
        }
        if args.contains("-develop-test") || args.contains("-mask-test") {
            model.live?.goToStep(applied: opened)
            model.refresh()
            print("TESTS DONE: back to the edit as opened (\(opened) steps applied)")
        }
        if args.contains("-loupe-test") { await loupeTest() }
        if args.contains("-ai-test"), let live = model.live {
            await aiTest(live)
        }
        if args.contains("-save") { model.save() }
        if let edit = model.edit {
            let exposure = edit.sections.flatMap(\.sliders).first { $0.label == "Exposure" }?.text ?? "?"
            print("EDIT exposure \(exposure); steps \(edit.steps); applied \(edit.applied); unsaved \(edit.needsSave)")
        }
    }

    /// `-develop-test`, continued: a built-in preset's Amount, a preset made,
    /// updated, renamed and deleted, and a snapshot made, applied and
    /// deleted, printing `PRESETS …` and `SNAPSHOTS …`. Leaves no preset or
    /// snapshot behind.
    private func presetsAndSnapshotsTest(_ live: LiveView) async {
        var amount = "none"
        for preset in presets().filter({ $0.id.hasPrefix("builtin:") }).prefix(12) {
            live.applyPreset(id: preset.id)
            for _ in 0..<30 where live.editState()?.presetAmount == nil { try? await Task.sleep(for: .milliseconds(100)) }
            if live.editState()?.presetAmount != nil {
                live.setPresetAmount(amount: 0.5, done: false)
                live.setPresetAmount(amount: 0.4, done: true)
                amount = "\(preset.name): \(live.editState()?.presetAmount?.amount ?? -1), last step \(live.editState()?.steps.last ?? "")"
                break
            }
        }
        let exposure = copyGroups().filter { $0.label == "Exposure" }.map(\.index)
        var made = "not made"
        do {
            let id = try live.createPreset(name: "Develop Test", group: "Safelight Test", groups: exposure)
            let listed = presets().first { $0.id == id }
            try live.updatePreset(id: id)
            try renamePreset(id: id, name: "Develop Test Renamed")
            let renamed = presets().first { $0.name == "Develop Test Renamed" }
            if let renamed { try deletePreset(id: renamed.id) }
            made = "listed \(listed.map { "\($0.group)/\($0.name) managed \($0.managed)" } ?? "no"); renamed \(renamed != nil); left \(presets().contains { $0.name.hasPrefix("Develop Test") })"
        } catch {
            made = "failed: \(error)"
        }
        print("PRESETS amount \(amount); made \(made); groups \(presetGroups())")
        do {
            let before = live.editState()?.sections.flatMap(\.sliders).first { $0.label == "Exposure" }?.text ?? "?"
            let id = try live.addSnapshot(name: nil)
            live.setControl(control: .param(index: 2), value: 1.5, done: true)
            try live.renameSnapshot(id: id, name: "Develop Test")
            _ = try live.applySnapshot(id: id)
            let after = live.editState()?.sections.flatMap(\.sliders).first { $0.label == "Exposure" }?.text ?? "?"
            let listed = live.snapshots().map(\.name)
            try live.deleteSnapshot(id: id)
            print("SNAPSHOTS \(listed); exposure \(before) -> \(after) after applying; last step \(live.editState()?.steps.last ?? ""); left \(live.snapshots().count)")
        } catch {
            print("SNAPSHOTS failed: \(error)")
        }
    }

    /// `-develop-test`, continued: each Before/After view, timed over a few
    /// frames, and Copy and Swap, printing `COMPARE …`. Ends on the edit
    /// alone, with what it copied taken back.
    private func beforeAfterTest(_ live: LiveView) async {
        var views: [String] = []
        let all: [(String, Compare)] = [
            ("before", .beforeOnly), ("left/right", .sideBySide(topBottom: false)),
            ("top/bottom", .sideBySide(topBottom: true)), ("split", .split(topBottom: false, position: 0.3)),
        ]
        for (i, (name, view)) in all.enumerated() {
            live.setCompare(compare: view)
            let first = live.stats().frames
            for _ in 0..<50 where live.stats().frames == first { try? await Task.sleep(for: .milliseconds(50)) }
            let opened = live.stats()
            // An edit beside Before: only the edit should render again.
            let exposure = 0.2 + 0.1 * Float(i)
            live.setControl(control: .param(index: 2), value: exposure, done: false)
            for _ in 0..<50 where live.stats().frames == opened.frames { try? await Task.sleep(for: .milliseconds(50)) }
            live.setControl(control: .param(index: 2), value: exposure, done: true)
            let edited = live.stats()
            views.append("\(name) \(Int(opened.renderMs)) ms, edited \(Int(edited.renderMs)) ms (frame \(edited.frames - opened.frames), \(edited.width)×\(edited.height))")
        }
        live.setCompare(compare: .off)
        let steps = live.editState()?.steps.count ?? 0
        live.copyBeforeAfter(copy: .afterToBefore)
        live.setControl(control: .param(index: 2), value: -1, done: true)
        live.copyBeforeAfter(copy: .swap)
        let swapped = live.editState()
        let exposure = swapped?.sections.flatMap(\.sliders).first { $0.label == "Exposure" }?.text ?? "?"
        print("COMPARE \(views.joined(separator: "; ")); after Swap exposure \(exposure), last step \(swapped?.steps.last ?? ""), \((swapped?.steps.count ?? 0) - steps) new steps")
        live.goToStep(applied: UInt32(steps))
    }

    /// `-loupe-test`: swipes to the next photo and back, as a swipe at Fit
    /// does, waiting each time for that photo's own edit. Prints `LOUPE …`.
    private func loupeTest() async {
        var visited: [String] = []
        for by in [1, -1] {
            let from = model.photo?.id
            let start = ContinuousClock.now
            model.onSwipe?(by)
            for _ in 0..<200 {
                model.refresh()
                if model.photo?.id != from, model.edit?.photo == model.photo?.id { break }
                try? await Task.sleep(for: .milliseconds(50))
            }
            let ms = (ContinuousClock.now - start).components
            visited.append("\(model.photo?.filename ?? "?") (edit of \(model.edit?.photo == model.photo?.id ? "it" : "another photo"), \(ms.seconds * 1000 + ms.attoseconds / 1_000_000_000_000_000) ms)")
        }
        print("LOUPE \(visited.joined(separator: " then "))")
    }

    /// `-ai-test`: installs any missing model, selects the subject and the
    /// sky as masks, then turns AI Denoise on and waits for its result,
    /// printing `AI …` lines with the times.
    private func aiTest(_ live: LiveView) async {
        for info in aiModels() where !info.installed {
            let start = Date()
            let install = ModelInstall.start(model: info.model)
            while !install.finished() { try? await Task.sleep(for: .milliseconds(500)) }
            print("AI install \(info.name): \(install.error() ?? "done") in \(Int(Date().timeIntervalSince(start))) s")
        }
        for what in [AiSelection.subject, .sky] {
            let start = Date()
            let outcome = await Task.detached { Result { try live.selectMask(what: what) } }.value
            let ms = Int(Date().timeIntervalSince(start) * 1000)
            switch outcome {
            case .success(let index): print("AI select \(what): mask \(index.map(String.init) ?? "none") in \(ms) ms, footprint \(Memory.mb(Memory.footprint().current)), peak \(Memory.mb(Memory.footprint().peak))")
            case .failure(let error): print("AI select \(what) failed in \(ms) ms: \(error)")
            }
        }
        let start = Date()
        live.setSwitch(which: .aiDenoise, on: true)
        var last = ""
        for _ in 0..<1200 {
            let status = live.editState()?.aiDenoise ?? "none"
            if status != last { print("AI denoise \(Int(Date().timeIntervalSince(start))) s: \(status), footprint \(Memory.mb(Memory.footprint().current)), peak \(Memory.mb(Memory.footprint().peak))"); last = status }
            // "none" until the full-size decode has rendered once.
            let running = status == "none" || status.hasPrefix("Waiting") || status.hasPrefix("Denoising")
            if !running { break }
            try? await Task.sleep(for: .milliseconds(250))
        }
        model.refresh()
        print("AI masks \(model.edit?.masks.map { "\($0.name)·\($0.kind)" } ?? []); steps \(model.edit?.steps ?? [])")
    }

    private var historyText: String {
        guard let edit = model.edit, edit.applied > 0 else { return "No changes" }
        return "\(edit.steps[Int(edit.applied) - 1]) · \(edit.applied) of \(edit.steps.count) steps"
    }

    @ViewBuilder private var saveStatus: some View {
        if let error = model.edit?.saveError {
            Label("Not saved: \(error)", systemImage: "exclamationmark.triangle")
                .labelStyle(.titleAndIcon).foregroundStyle(.red).font(.caption).lineLimit(1)
        } else if model.edit?.needsSave ?? false {
            Label("Unsaved", systemImage: "circle.dotted").labelStyle(.titleAndIcon).font(.caption).foregroundStyle(.secondary)
        } else if model.edit != nil {
            Label("Saved", systemImage: "checkmark.circle").labelStyle(.titleAndIcon).font(.caption).foregroundStyle(.secondary)
        }
    }

    private func summary(_ s: FrameStats) -> String {
        if let error = s.error { return "Error: \(error)" }
        if s.loading { return "Opening and decoding…" }
        if s.frames == 0 { return "Choose a photo." }
        let memory = Memory.footprint()
        let zoom = if case .actual = s.zoom { "100%" } else { "Fit" }
        return """
        \(s.camera) · decode \(Int(s.decodeMs)) ms · \(zoom) · \(s.backend) \(s.width)×\(s.height) of \(s.fullWidth)×\(s.fullHeight) · frames \(s.frames)
        render \(Int(s.renderMs)) ms (mean of 30: \(Int(s.meanRenderMs)) ms) · draw \(String(format: "%.1f", s.presentMs)) ms · memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak)), available \(Memory.mb(Memory.available()))
        """
    }

    /// Drags Exposure from -2 to +2 and back at 60 steps a second, for 4
    /// seconds, then prints what it cost: the `-sweep` launch option.
    private func sweep(_ live: LiveView) {
        Task.detached {
            while live.stats().frames == 0 {
                if let error = live.stats().error { print("SWEEP FAILED: \(error)"); return }
                try? await Task.sleep(for: .milliseconds(100))
            }
            let start = live.stats()
            let clock = ContinuousClock.now
            let steps = 240
            for i in 0..<steps {
                let phase = Double(i) / Double(steps)
                let value = Float(phase < 0.5 ? -2 + 8 * phase : 6 - 8 * phase)
                live.setControl(control: .param(index: 2), value: value, done: i == steps - 1)
                try? await Task.sleep(for: .milliseconds(16))
            }
            try? await Task.sleep(for: .milliseconds(500))
            let end = live.stats()
            let seconds = Double((ContinuousClock.now - clock).components.seconds)
            let frames = end.frames - start.frames
            let memory = Memory.footprint()
            print("""
            SWEEP \(end.camera) [\(end.backend) \(end.width)×\(end.height)]: \(steps) slider values in \(String(format: "%.1f", seconds)) s → \(frames) frames, \
            \(String(format: "%.1f", Double(frames) / max(seconds, 0.001))) fps; mean render \(Int(end.meanRenderMs)) ms, last draw \(String(format: "%.1f", end.presentMs)) ms; \
            decode \(Int(end.decodeMs)) ms; memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak)), available \(Memory.mb(Memory.available()))
            SWEEP DONE
            """)
        }
    }
}

extension LiveEditor {
    /// At 100%, pans across the photo's middle from 30% to 70% of its width
    /// and back at 60 steps a second for 4 seconds, then prints what it
    /// cost: the `-pan` launch option.
    func pan(_ live: LiveView) {
        Task.detached {
            while live.stats().frames == 0 {
                if let error = live.stats().error { print("PAN FAILED: \(error)"); return }
                try? await Task.sleep(for: .milliseconds(100))
            }
            live.setZoom(zoom: .actual(centerX: 0.3, centerY: 0.5))
            let first = live.stats().frames
            while live.stats().frames == first { try? await Task.sleep(for: .milliseconds(20)) }
            let opened = live.stats()
            let start = live.stats()
            let clock = ContinuousClock.now
            let steps = 240
            for i in 0..<steps {
                let phase = Double(i) / Double(steps)
                let x = Float(phase < 0.5 ? 0.3 + 0.8 * phase : 1.1 - 0.8 * phase)
                live.setZoom(zoom: .actual(centerX: x, centerY: 0.5))
                try? await Task.sleep(for: .milliseconds(16))
            }
            try? await Task.sleep(for: .milliseconds(500))
            let end = live.stats()
            let seconds = Double((ContinuousClock.now - clock).components.seconds)
            let frames = end.frames - start.frames
            let memory = Memory.footprint()
            print("""
            PAN \(end.camera) [\(end.backend) region \(end.width)×\(end.height) of \(end.fullWidth)×\(end.fullHeight)]: first 100% frame \(Int(opened.renderMs)) ms; \
            \(steps) positions in \(String(format: "%.1f", seconds)) s → \(frames) frames, \(String(format: "%.1f", Double(frames) / max(seconds, 0.001))) fps; \
            mean render \(Int(end.meanRenderMs)) ms; memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak)), available \(Memory.mb(Memory.available()))
            PAN DONE
            """)
        }
    }
}

extension FrameStats {
    static let empty = FrameStats(
        frames: 0, renderMs: 0, presentMs: 0, meanRenderMs: 0, backend: "", width: 0, height: 0,
        fullWidth: 0, fullHeight: 0, frameWidth: 0, frameHeight: 0, zoom: .fit, camera: "", decodeMs: 0, loading: false, error: nil)
}

/// Every Develop panel the core describes, in the desktop's order: each a
/// section that folds, with its switches and sliders.
struct DevelopPanels: View {
    let model: LiveModel
    @State private var open: Set<String> = ["Basic"]

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 8) {
                HistogramPanel(model: model)
                SnapshotsPanel(model: model)
                HistoryPanel(model: model)
                Divider()
                PresetsPanel(model: model)
                Divider()
                MasksPanel(model: model)
                Divider()
                ForEach(model.edit?.sections ?? [], id: \.title) { section in
                    PanelSection(section: section, model: model, isOpen: isOpen(section.title))
                    if section.title == "AI Denoise" {
                        AiDenoiseStatus(model: model)
                    }
                    Divider()
                }
            }
            .padding(.horizontal, 4)
        }
    }

    private func isOpen(_ title: String) -> Binding<Bool> {
        Binding(
            get: { open.contains(title) },
            set: { shown in
                if shown { open.insert(title) } else { open.remove(title) }
            }
        )
    }
}

struct PanelSection: View {
    let section: Section
    let model: LiveModel
    @Binding var isOpen: Bool

    var body: some View {
        DisclosureGroup(isExpanded: $isOpen) {
            VStack(alignment: .leading, spacing: 6) {
                if section.title == "Basic" {
                    BasicExtras(model: model)
                }
                if section.title == "Tone Curve" {
                    PointCurveEditor(model: model)
                }
                ForEach(section.switches, id: \.label) { item in
                    SwitchRow(item: item, model: model)
                }
                ForEach(section.sliders, id: \.label) { slider in
                    SliderRow(slider: slider, model: model)
                }
            }
            .padding(.top, 4)
        } label: {
            Text(section.title).font(.headline)
        }
    }
}

struct SwitchRow: View {
    let item: SwitchState
    let model: LiveModel

    var body: some View {
        SwiftUI.Toggle(item.label, isOn: Binding(
            get: { item.on },
            set: { model.setSwitch(item.id, on: $0) }
        ))
        .font(.callout)
    }
}

/// One slider: label, track and value, as the desktop's.
struct SliderRow: View {
    let slider: SliderState
    let model: LiveModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(slider.label).font(.callout)
                Spacer()
                Text(slider.text).font(.callout.monospacedDigit()).foregroundStyle(.secondary)
            }
            Slider(
                value: Binding(
                    get: { shown },
                    set: { model.set(slider.control, to: snapped($0), done: false) }
                ),
                in: slider.min...slider.max
            ) { editing in
                if !editing { model.set(slider.control, to: shown, done: true) }
            }
        }
    }

    /// The value being dragged, else the edit's.
    private var shown: Float {
        if let (control, value) = model.dragging, control == slider.control { return value }
        return slider.value
    }

    /// Snapped to the model's drag step (0.05 EV for Exposure), if it has one.
    private func snapped(_ value: Float) -> Float {
        guard slider.step > 0 else { return value }
        return (value / slider.step).rounded() * slider.step
    }
}
