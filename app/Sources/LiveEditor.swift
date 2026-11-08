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
    /// The right-hand panel's tool. Crop shows the crop's handles; leaving
    /// Masks or Edit lets go of a mask or the eyedropper.
    /// History's steps applied when a tool's pane opened (Crop, Masks,
    /// Heal), for its Cancel to go back to.
    var toolStart: UInt32?
    var pane: DevelopPane = .edit {
        didSet {
            guard pane != oldValue else { return }
            toolStart = pane.isTool ? edit?.applied : nil
            cropping = pane == .crop
            if pane != .crop { cropRuler = false; straightenNote = nil }
            if pane != .edit { targeted = nil }
            if pane != .masks { selectedMask = nil; if tool != .whiteBalance && !(pane == .heal && tool == .heal) { tool = .none } }
            if pane != .edit, tool == .whiteBalance || tool == .pointColor { tool = .none }
        }
    }
    /// A tool's Cancel: back to the edit as it was when the tool opened,
    /// then to Edit. The steps it took stay in History, as after an Undo,
    /// until the next change.
    func cancelTool() {
        if let start = toolStart { live?.goToStep(applied: start) }
        refresh()
        pane = .edit
    }

    /// A tool's Apply: keeps what it did and goes back to Edit.
    func applyTool() { pane = .edit }

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
    /// The Crop tool's Straighten ruler is out: a drag over the photo draws a
    /// line to level instead of moving the crop.
    var cropRuler = false
    /// Auto Straighten is measuring, or why it found nothing.
    var straightenNote: String?
    /// The Crop tool's guide overlay (an index of `cropGuides()`, Thirds to
    /// start, as Lightroom's), `nil` for none, and which way round it is.
    var cropGuide: UInt32? = 1
    var cropGuideOrientation: UInt32 = 0
    /// The Targeted Adjustment Tool, on for these sliders, and why its last
    /// drag moved nothing.
    var targeted: TargetKind? { didSet { if targeted != nil, tool != .none { tool = .none } } }
    var targetedNote: String?
    /// The Point Color swatch whose sliders show.
    var pointColorSelected: UInt32?
    /// Why the last sample made no swatch, or that it is being taken.
    var pointColorNote: String?
    var brushSize: Float = 0.04
    var brushFeather: Float = 0.5
    var brushFlow: Float = 1
    var brushErase = false
    /// What overlapping strokes build up to, as Lightroom's Density.
    var brushDensity: Float = 1
    var brushAutoMask = false
    /// A Subject refine's taps so far (x, y pairs) and whether each adds.
    var refineTaps: [Float] = []
    var refineAdds: [Bool] = []
    /// Whether the next refine tap adds or leaves out.
    var refineAdd = true
    var refining = false
    var refineNote: String?
    var spotSize: Float = 0.02
    var cloneSpots = false
    /// A Select Subject, Sky or Background run in progress, and why the last
    /// one failed.
    var selecting: AiSelection?
    /// An Auto estimate running.
    var autoRunning: AutoKind?
    /// Show the selected mask over the photo, in its colour, as
    /// Lightroom's overlay.
    var showMaskOverlay = true
    /// Each mask's overlay colour, by index, as chosen in the Masks panel.
    var maskColors: [UInt32: MaskColor] = [:]
    /// Each mask's coverage drawn small, for the list beside the photo.
    var maskThumbs: [UInt32: UIImage] = [:]
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
        if pane == .crop { pane = .edit }
        cropping = false
        tool = .none
        selectedMask = nil
        maskColors = [:]
        maskThumbs = [:]
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
        // A mask's overlay would hide what its sliders do, colour above all:
        // once one moves it goes off, as in Lightroom, until shown again.
        switch control { case .mask, .maskAmount: showMaskOverlay = false; default: break }
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
        case .scaled(let z, let cx, let cy):
            let z = CGFloat(z)
            return (cx + Float((x - w / 2) / (fw * z)), cy + Float((y - h / 2) / (fh * z)))
        }
    }

    /// Screen pixels per photo pixel now: Fit's, 1 at 100%, or the zoom's.
    func zoomScale(in size: CGSize, scale: CGFloat) -> CGFloat? {
        guard let stats = live?.stats(), stats.fullWidth > 0 else { return nil }
        let fit = min(size.width * scale / CGFloat(stats.fullWidth), size.height * scale / CGFloat(stats.fullHeight))
        switch zoom {
        case .fit: return fit
        case .actual: return 1
        case .scaled(let z, _, _): return CGFloat(z)
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

    /// Ends a pinch at the scale it reached, between Fit and 100%, centred
    /// on its anchor: past either end it settles there.
    func endPinch(in size: CGSize, scale: CGFloat) {
        defer { pinch = 1 }
        guard !cropping && tool == .none, let stats = live?.stats(), stats.fullWidth > 0,
              let now = zoomScale(in: size, scale: scale) else { return }
        let fit = min(size.width * scale / CGFloat(stats.fullWidth), size.height * scale / CGFloat(stats.fullHeight))
        let target = now * pinch
        let anchor = CGPoint(x: pinchAnchor.x * size.width, y: pinchAnchor.y * size.height)
        guard let (x, y) = photoPoint(at: anchor, in: size, scale: scale) else { return }
        if target <= fit * 1.02 || fit >= 1 {
            zoom = target >= 1 ? .actual(centerX: x, centerY: y) : .fit
        } else if target >= 0.98 {
            zoom = .actual(centerX: x, centerY: y)
        } else {
            zoom = .scaled(scale: Float(target), centerX: x, centerY: y)
        }
    }

    /// A swipe across the photo at Fit, with no tool open: the previous or
    /// next photo, as Lightroom's loupe.
    func swipe(by translation: CGSize) {
        guard !cropping && tool == .none, case .fit = zoom, compare == .off || compare == .beforeOnly else { return }
        guard abs(translation.width) > 80, abs(translation.width) > 2 * abs(translation.height) else { return }
        onSwipe?(translation.width < 0 ? 1 : -1)
    }

    /// Pans a zoomed view by `translation` points from where the drag started.
    func drag(by translation: CGSize, scale: CGFloat) {
        guard !cropping && tool == .none, let stats = live?.stats(), stats.fullWidth > 0 else { return }
        let (cx, cy, z): (Float, Float, CGFloat)
        switch zoom {
        case .fit: return
        case .actual(let x, let y): (cx, cy, z) = (x, y, 1)
        case .scaled(let s, let x, let y): (cx, cy, z) = (x, y, CGFloat(s))
        }
        if dragStart == nil { dragStart = (cx, cy) }
        let (sx, sy) = dragStart!
        let x = sx - Float(translation.width * scale / (CGFloat(stats.fullWidth) * z))
        let y = sy - Float(translation.height * scale / (CGFloat(stats.fullHeight) * z))
        if case .scaled(let s, _, _) = zoom {
            zoom = .scaled(scale: s, centerX: x, centerY: y)
        } else {
            zoom = .actual(centerX: x, centerY: y)
        }
    }

    /// Ends a drag on the centre the core kept inside the photo.
    func endDrag() {
        dragStart = nil
        switch live?.stats().zoom {
        case .some(.actual(let x, let y)): if case .actual = zoom { zoom = .actual(centerX: x, centerY: y) }
        case .some(.scaled(let s, let x, let y)): if case .scaled = zoom { zoom = .scaled(scale: s, centerX: x, centerY: y) }
        default: break
        }
    }
}

/// Stage 3: the photo drawn live by the core, with an Exposure slider.
struct LiveEditor: View {
    /// The launch options' tests have run (see `scripted`).
    @MainActor private static var scriptedRan = false
    let photo: LibraryPhoto
    let library: Library
    let model: LiveModel
    var rating: Int32 = 0
    var flag: Int32 = 0
    var setRating: (Int32) -> Void = { _ in }
    var setFlag: (Int32) -> Void = { _ in }
    var showInfo: () -> Void = {}
    @Environment(\.displayScale) private var displayScale


    var body: some View {
        HStack(spacing: 0) {
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
                    } else if let kind = model.targeted {
                        TargetedOverlay(model: model, kind: kind, size: geometry.size)
                    } else if model.tool.samples {
                        SampleLoupe(model: model, size: geometry.size)
                    } else if model.tool != .none {
                        PhotoToolOverlay(model: model, size: geometry.size)
                    } else if model.compare != .off {
                        CompareOverlay(model: model, size: geometry.size)
                    } else if model.selectedMask != nil, case .fit = model.zoom {
                        MaskOnPhoto(model: model, size: geometry.size)
                    }
                }
            }
            .background(Theme.canvas)
            .overlay(alignment: .bottomLeading) { status.padding(12) }
            .overlay(alignment: .bottomTrailing) {
                if model.pane == .masks && model.compare == .off {
                    MaskStrip(model: model).padding(12)
                }
            }
            .overlay(alignment: .topLeading) {
                // Over the photo's corner, as Lightroom shows it while editing.
                if model.pane == .edit && model.compare == .off {
                    HistogramPanel(model: model).frame(width: 230).padding(12)
                }
            }
            DevelopPanel(model: model)
            ToolRail(model: model, rating: rating, flag: flag, setRating: setRating, setFlag: setFlag, showInfo: showInfo)
        }
        .background(Theme.canvas)
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
        // Once per launch: the editor stays as photos change under it, and a
        // test must never run again on the next photo.
        guard !Self.scriptedRan else { return }
        Self.scriptedRan = true
        for _ in 0..<300 where model.edit == nil {
            model.refresh()
            try? await Task.sleep(for: .milliseconds(100))
        }
        let args = ProcessInfo.processInfo.arguments
        // The edit as opened: the tests below go back to it when they're
        // done, as the app saves whatever is shown when it is ended.
        let opened = model.edit?.applied ?? 0
        // -reset and -clear-history: back to the starting settings, then no
        // History, before any -set-exposure: for putting a photo back.
        if args.contains("-reset") { model.live?.reset() }
        if args.contains("-clear-history") { model.live?.clearHistory() }
        if let i = args.firstIndex(of: "-set-exposure"), i + 1 < args.count, let stops = Float(args[i + 1]) {
            model.set(.param(index: 2), to: stops, done: true)
        }
        if args.contains("-mask-test"), let live = model.live {
            // A radial mask in the middle at +1 EV, a brush stroke, a Heal spot.
            if let radial = live.addMask(kind: .radial, from: [0.5, 0.5], to: [0.65, 0.6]) {
                if let exposure = live.maskSliders(index: radial).first(where: { $0.label == "Exposure" }) {
                    live.setControl(control: exposure.control, value: 1, done: true)
                }
            }
            if let brush = live.addMask(kind: .brush, from: [0, 0], to: [0, 0]) {
                live.brush(index: brush, stroke: Stroke(points: [0.2, 0.3, 0.3, 0.35, 0.4, 0.4], radius: 0.03, feather: 0.5, flow: 0.8, density: 1, erase: false, autoMask: false))
            }
            let healed = live.addSpot(at: [0.7, 0.7], radius: 0.02, clone: false)
            model.refresh()
            print("MASKS \(model.edit?.masks.map { "\($0.name)·\($0.kind)" } ?? []); spots \(model.edit?.spots ?? 0) (heal found a source: \(healed)); steps \(model.edit?.steps ?? [])")
        }
        // -import-profiles <folder in Documents>: imports it as Import
        // Profiles does, and lists the photo's profiles again.
        if let i = args.firstIndex(of: "-import-profiles"), i + 1 < args.count, let live = model.live {
            let folder = URL.documentsDirectory.appending(path: args[i + 1])
            let before = live.profiles().count
            let result = await Task.detached { () -> ImportSummary in
                let r = importProfiles(paths: [folder.path])
                live.reloadProfiles()
                return r
            }.value
            model.refresh()
            let looks = live.profiles().filter { $0.title.contains("Teal") }.map { "\($0.group)/\($0.title)" }
            print("PROFILES IMPORT \(result.message); failed \(result.failed) \(result.details); profiles \(before) → \(live.profiles().count); \(looks)")
        }
        // -crop-guide <n>: the Crop tool open with overlay n of cropGuides().
        if let i = args.firstIndex(of: "-crop-guide"), i + 1 < args.count, let n = UInt32(args[i + 1]) {
            model.pane = .crop
            model.cropGuide = n
        }
        // -targeted (with -color-mix): the Targeted Adjustment Tool on, for
        // the Color Mixer's Saturation.
        if args.contains("-targeted") { model.targeted = .saturation }
        // -wb-loupe: the white balance eyedropper open, to look at.
        if args.contains("-wb-loupe") { model.tool = .whiteBalance }
        // -point-color (with -color-mix): Point Color's eyedropper open;
        // -point-color-ranges: a swatch sampled at the middle of the photo,
        // its ranges and Visualize Range shown.
        if args.contains("-point-color-ranges"), let live = model.live {
            let at: [Float] = [0.555, 0.49]
            let added = await Task.detached { Result { try live.addPointColor(at: at) } }.value
            print("POINT COLOR added \(added); ranges \(live.pointColors().first?.ranges.map { "\($0.label) \($0.points)" } ?? [])")
            model.refresh()
        } else if args.contains("-point-color") {
            model.tool = .pointColor
        }
        // -show-mask: the Masks pane on the first mask, its Color open.
        if args.contains("-show-mask"), let first = model.edit?.masks.first?.index {
            model.pane = .masks
            model.selectedMask = first
        }
        if args.contains("-mask-color-test"), let live = model.live {
            // A radial mask over the whole photo; each Color slider in turn
            // must change the live histogram, then goes back.
            func frame() async -> [UInt32]? {
                let frames = live.stats().frames
                for _ in 0..<50 where live.stats().frames == frames { try? await Task.sleep(for: .milliseconds(100)) }
                let h = live.histogram()
                return h.red + h.green + h.blue
            }
            if let mask = live.addMask(kind: .radial, from: [0.5, 0.5], to: [1.5, 1.5]) {
                let base = await frame()
                for slider in (live.maskSections(index: mask).first { $0.title == "Color" }?.sliders ?? []) {
                    live.setControl(control: slider.control, value: slider.label == "Hue" ? 90 : slider.max, done: true)
                    let after = await frame()
                    print("MASK COLOR \(slider.label) → \(live.maskSliders(index: mask).first { $0.label == slider.label }?.text ?? "?"): \(after == base ? "NO CHANGE" : "changed")")
                    live.undo()
                    _ = await frame()
                }
            }
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
                let amount = live.editState()?.sections.first { $0.group == "Profile" }?.sliders.first?.label ?? "none"
                print("PROFILES \(profiles.count) in \(Set(profiles.map(\.group)).count - 1) groups; chose \(other.title); Profile Amount slider \(amount); last step \(live.editState()?.steps.last ?? "")")
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
        // -keep leaves what the tests did, to look at; never on a photo you
        // keep, as ending the app saves it.
        if (args.contains("-develop-test") || args.contains("-mask-test") || args.contains("-mask-color-test")) && !args.contains("-keep") {
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

    /// Over the photo's corner: whether the edit is saved, and with
    /// `-stats`, what rendering costs. Also picks up the edit once it has
    /// loaded, and AI Denoise's progress, which arrive from the core on
    /// their own.
    private var status: some View {
        TimelineView(.periodic(from: .now, by: 0.25)) { _ in
            let _ = (model.edit == nil || model.edit?.aiDenoise != nil) ? model.refresh() : ()
            VStack(alignment: .leading, spacing: 4) {
                if ProcessInfo.processInfo.arguments.contains("-stats") {
                    Text(summary(model.live?.stats() ?? .empty)).font(.caption2.monospaced())
                }
                saveStatus
            }
            .foregroundStyle(Theme.secondaryText)
        }
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
        let zoom = switch s.zoom {
        case .actual: "100%"
        case .scaled(let z, _, _): "\(Int((z * 100).rounded()))%"
        case .fit: "Fit"
        }
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
    var track: [Color] = []
    /// Shown in place of the slider's own label ("Hue" for "Red Hue").
    var label: String?

    /// The colours along a slider's track, where they say what it does:
    /// white balance's and saturation's, as Lightroom draws them.
    static func track(for label: String, in section: String) -> [Color] {
        guard section == "Color" else { return [] }
        switch label {
        case "Temp": return [Color(red: 0.25, green: 0.45, blue: 0.95), Color(red: 0.95, green: 0.85, blue: 0.3)]
        case "Tint": return [Color(red: 0.3, green: 0.75, blue: 0.3), Color(red: 0.85, green: 0.3, blue: 0.8)]
        case "Vibrance", "Saturation": return [Color(white: 0.5), Color(red: 0.9, green: 0.35, blue: 0.2)]
        // A mask's Hue shifts round the wheel.
        case "Hue": return [.red, .yellow, .green, .cyan, .blue, .purple, .red]
        default: return []
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(label ?? slider.label).font(.callout).foregroundStyle(Theme.secondaryText)
                Spacer()
                Text(slider.text).font(.callout.monospacedDigit())
            }
            ThinSlider(value: shown, range: slider.min...slider.max, track: track, reset: reset) { value, done in
                model.set(slider.control, to: snapped(value), done: done)
            }
        }
        .contentShape(Rectangle())
        // On the label too, as on the track.
        .onTapGesture(count: 2) { reset() }
    }

    /// Back to where the photo started: Temp and Tint as shot, a mask's
    /// sliders as a new mask's.
    private func reset() {
        model.set(slider.control, to: slider.reset, done: true)
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
