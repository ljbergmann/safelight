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
    var cropping = false { didSet { live?.setCropping(on: cropping); zoom = .fit } }
    var cropAspect: CropAspect = .free
    /// The chosen ratio turned to portrait (the crop bar's Swap).
    var cropPortrait = false
    /// Settings were copied in this editor (the catalog may also hold some).
    var copied = false
    var copying = false
    var exporting = false
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
        zoom = .fit
        live.openPhoto(library: library, id: photo.id, path: photo.path, decode: size)
    }

    /// Reads the edit back from the core.
    func refresh() {
        edit = live?.editState()
    }

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
        let (w, h) = (size.width * scale, size.height * scale)
        let (x, y) = (location.x * scale, location.y * scale)
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
        guard !cropping else { return }
        if case .fit = zoom, let (x, y) = photoPoint(at: location, in: size, scale: scale) {
            zoom = .actual(centerX: x, centerY: y)
        } else {
            zoom = .fit
        }
    }

    /// Ends a pinch: in from Fit to 100% around its anchor, or out to Fit.
    func endPinch(in size: CGSize, scale: CGFloat) {
        guard !cropping else { pinch = 1; return }
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

    /// Pans at 100% by `translation` points from where the drag started.
    func drag(by translation: CGSize, scale: CGFloat) {
        guard !cropping else { return }
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
                        .onEnded { _ in model.endDrag() }
                )
                .onTapGesture(count: 2) { location in
                    model.toggleZoom(at: location, in: geometry.size, scale: displayScale)
                }
                .overlay {
                    if model.cropping { CropOverlay(model: model, size: geometry.size) }
                }
            }
            .background(Color(white: 0.11))
            .clipShape(RoundedRectangle(cornerRadius: 8))

            if model.cropping { CropBar(model: model) }

            HStack(spacing: 18) {
                Button("Crop", systemImage: "crop") { model.cropping.toggle() }
                    .foregroundStyle(model.cropping ? Color.accentColor : .primary)
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
                let _ = model.edit == nil ? model.refresh() : ()
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
        .task { await scripted() }
        .onAppear {
            // -full: decode at full size from the start, for -sweep.
            if ProcessInfo.processInfo.arguments.contains("-full") { model.size = .full }
            model.library = library
            model.photo = photo
        }
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
        if let i = args.firstIndex(of: "-set-exposure"), i + 1 < args.count, let stops = Float(args[i + 1]) {
            model.set(.param(index: 2), to: stops, done: true)
        }
        if args.contains("-save") { model.save() }
        if let edit = model.edit {
            let exposure = edit.sections.flatMap(\.sliders).first { $0.label == "Exposure" }?.text ?? "?"
            print("EDIT exposure \(exposure); steps \(edit.steps); applied \(edit.applied); unsaved \(edit.needsSave)")
        }
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
                PresetsPanel(model: model)
                Divider()
                ForEach(model.edit?.sections ?? [], id: \.title) { section in
                    PanelSection(section: section, model: model, isOpen: isOpen(section.title))
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
