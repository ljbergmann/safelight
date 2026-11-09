import SwiftUI
import UniformTypeIdentifiers

/// The catalog's photos as a grid; tap one to edit it.
struct LibraryView: View {
    @State private var model = LibraryModel()
    @State private var path: [LibraryPhoto.ID] = []
    @State private var picking = false
    @State private var pickingPhotos = false
    /// What the Files picker picks: a card or folder for the import
    /// screen, or photo files to import straight away.
    @State private var pickFolder = false
    /// The card or folder the import screen shows.
    @State private var importing: ImportChoice?
    /// Select mode: taps choose photos instead of opening them.
    @State private var selecting = false
    @State private var selected: Set<Int64> = []
    @State private var pasteNote: String?
    @State private var exporting = false
    /// The catalog sidebar, hidden while a photo is open in the editor.
    @State private var columns: NavigationSplitViewVisibility = .all
    /// On an iPhone, which column shows: the grid, with the sidebar behind it.
    @State private var compactColumn: NavigationSplitViewColumn = .detail

    var body: some View {
        NavigationSplitView(columnVisibility: $columns, preferredCompactColumn: $compactColumn) {
            LibrarySidebar(model: model) { compactColumn = .detail }
                .navigationSplitViewColumnWidth(min: 260, ideal: 300, max: 340)
        } detail: {
        NavigationStack(path: $path) {
            GeometryReader { geometry in
                ScrollView {
                    MosaicGrid(model: model, selecting: selecting, selected: $selected, width: geometry.size.width)
                        .padding(.bottom, 90)
                }
                .scrollContentBackground(.hidden)
            }
            .background(Theme.canvas)
            .overlay(alignment: .bottom) { bottomPill.padding(.bottom, 18) }
            .overlay {
                // A card in the middle while photos come in: on an empty
                // library the grid is blank until they do.
                if let importing = model.importing { importCard(importing) }
            }
            .overlay {
                if model.photos.isEmpty && model.importing == nil {
                    ContentUnavailableView {
                        Label("No photos yet", systemImage: "photo.on.rectangle")
                    } description: {
                        Text("Import RAW files from the Files app or the Photos library. They're copied into the catalog.")
                    } actions: {
                        Button("From a Card or Drive…") { pickFolder = true; picking = true }.buttonStyle(.borderedProminent)
                        Button("From Files…") { pickFolder = false; picking = true }
                        Button("From Photos…") { pickingPhotos = true }
                    }
                }
            }
            .navigationTitle(model.source.title(in: model))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Theme.canvas, for: .navigationBar)
            .toolbar {
                ToolbarItemGroup(placement: .primaryAction) {
                    FilterMenu(model: model)
                    SortMenu(model: model)
                }
                if selecting {
                    ToolbarItem(placement: .primaryAction) {
                        Button("Info", systemImage: "info.circle") { model.info = InfoRequest(ids: Array(selected)) }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Menu("Files", systemImage: "folder") { FileMenu(ids: Array(selected), photo: nil, model: model) }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Menu("Albums", systemImage: "rectangle.stack") { AlbumMenu(ids: Array(selected), model: model) }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Menu("Stack", systemImage: "square.stack") { SelectionStackMenu(ids: Array(selected), model: model) }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Menu("Compare", systemImage: "rectangle.split.2x1") {
                            Button("Compare", systemImage: "rectangle.split.2x1") { choose(.compare) }
                                .disabled(selected.count < 2)
                            Button("Survey", systemImage: "square.grid.2x2") { choose(.survey) }
                                .disabled(selected.count < 2)
                        }
                        .disabled(selected.count < 2)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Button("Export", systemImage: "square.and.arrow.up") { exporting = true }
                            .disabled(selected.isEmpty)
                    }
                    ToolbarItem(placement: .primaryAction) {
                        Button("Paste Settings", systemImage: "doc.on.clipboard") {
                            let ids = Array(selected)
                            let notes = (try? model.library?.pasteSettings(ids: ids)) ?? []
                            pasteNote = notes.isEmpty ? "Pasted onto \(ids.count) photos" : notes.joined(separator: " · ")
                            selecting = false
                            selected = []
                        }
                        .disabled(selected.isEmpty || !(model.library?.hasCopiedSettings() ?? false))
                    }
                }
            }
            .sheet(isPresented: $exporting) {
                if let library = model.library {
                    ExportSheet(library: library, ids: model.shown.map(\.id).filter(selected.contains))
                }
            }
            .fileImporter(
                isPresented: $picking,
                allowedContentTypes: pickFolder ? [.folder] : [.rawImage, .image],
                allowsMultipleSelection: !pickFolder
            ) { result in
                guard case .success(let files) = result else { return }
                if pickFolder, let folder = files.first {
                    // Kept open while the screen lists it; the import opens it again.
                    _ = folder.startAccessingSecurityScopedResource()
                    importing = ImportChoice(folder: folder)
                } else {
                    Task { await model.importFiles(files) }
                }
            }
            .sheet(item: $importing, onDismiss: nil) { choice in
                ImportScreen(model: model, choice: choice)
                    .onDisappear { choice.folder.stopAccessingSecurityScopedResource() }
            }
            .sheet(isPresented: $pickingPhotos) {
                PhotosRawPicker { results in
                    pickingPhotos = false
                    Task { await model.importFromPhotos(results) }
                }
                .ignoresSafeArea()
            }
            .alert("Something went wrong", isPresented: .constant(model.error != nil)) {
                Button("OK") {}
            } message: {
                Text(model.error ?? "")
            }
            .navigationDestination(for: LibraryPhoto.ID.self) { id in
                if let photo = model.photos.first(where: { $0.id == id }) {
                    EditorView(photo: photo, model: model)
                }
            }
        }
        }
        .navigationSplitViewStyle(.balanced)
        .sheet(item: Bindable(model).info) { request in InfoSheet(model: model, ids: request.ids) }
        .fullScreenCover(item: Bindable(model).choosing) { request in
            switch request.mode {
            case .compare: CompareView(model: model, ids: request.ids)
            case .survey: SurveyView(model: model, ids: request.ids)
            }
        }
        .modifier(FileAlerts(model: model))
        .modifier(AlbumAlerts(model: model))
        .modifier(CopyAlerts(model: model, renaming: Bindable(model).renamingCopy, removing: Bindable(model).removingCopy))
        .onChange(of: path) { _, open in columns = open.isEmpty ? .all : .detailOnly }
        .task {
            await model.open()
            let args = ProcessInfo.processInfo.arguments
            if args.contains("-import-documents") { await model.importDocuments() }
            if args.contains("-photos-import-test") { await model.photosImportTest() }
            if args.contains("-import-screen-test") { await model.importScreenTest() }
            if args.contains("-sort-test") { model.sortTest() }
            if args.contains("-metadata-test") { model.metadataTest() }
            if args.contains("-virtual-copy-test") { await model.virtualCopyTest() }
            if args.contains("-file-ops-test") { await model.fileOpsTest() }
            if args.contains("-stack-test") { await model.stackTest() }
            // -survey / -compare: open on four photos, to look at, passing
            // over the test charts, which have no preview.
            let four = Array(model.shown.filter { !$0.path.hasSuffix("-d65.dng") }.prefix(4)).map(\.id)
            if args.contains("-survey") { model.choosing = ChoosingRequest(mode: .survey, ids: four) }
            if args.contains("-compare") { model.choosing = ChoosingRequest(mode: .compare, ids: four) }
            model.rateFromArguments()
            // -open <file name>: start in the editor, for -sweep and -pan.
            if let i = args.firstIndex(of: "-open"), i + 1 < args.count,
               let photo = model.photos.first(where: { $0.filename == args[i + 1] }) {
                path = [photo.id]
            }
            if args.contains("-benchmark") { Diagnostics.benchmark() }
            // -denoise-measure <file in Documents>: AI Denoise on the CPU and on each
            // CoreML plan, timed and compared (a development measurement).
            if let i = args.firstIndex(of: "-denoise-measure"), i + 1 < args.count {
                let raw = URL.documentsDirectory.appending(path: args[i + 1]).path
                let scratch = URL.temporaryDirectory.appending(path: "denoise-measure").path
                let report = await Task.detached { measureDenoiseProviders(raw: raw, model: nil, scratch: scratch) }.value
                print("DENOISE \(report)")
            }
            // -selection-measure <file in Documents>: Select Subject and Select Sky
            // on the CPU and on CoreML, timed and compared (a development measurement).
            if let i = args.firstIndex(of: "-selection-measure"), i + 1 < args.count {
                let raw = URL.documentsDirectory.appending(path: args[i + 1]).path
                let report = await Task.detached { measureSelectionProviders(raw: raw) }.value
                print("SELECT \(report)")
            }
            // -export <file name>: a full-size JPEG of it, timed, for measuring.
            if let i = args.firstIndex(of: "-export"), i + 1 < args.count,
               let photo = model.photos.first(where: { $0.filename == args[i + 1] }), let library = model.library {
                let start = ContinuousClock.now
                let folder = URL.temporaryDirectory.appending(path: "Export-test").path
                // The desktop's defaults, at full size: a JPEG named as its RAW.
                var choice = defaultExportChoice()
                (choice.format, choice.quality, choice.longEdge, choice.watermark) = (.jpeg, 90, 0, false)
                let outcome = await Task.detached { [choice] in
                    Result { try library.exportPhotos(ids: [photo.id], choice: choice, folder: folder).files }
                }.value
                let memory = Memory.footprint()
                switch outcome {
                case .success(let files):
                    let size = (try? FileManager.default.attributesOfItem(atPath: files[0])[.size] as? Int) ?? 0
                    print("EXPORT \(files[0].split(separator: "/").last ?? "") \(size / 1024) KB in \(start.duration(to: .now)); memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak))")
                case .failure(let e): print("EXPORT FAILED \(e)")
                }
            }
            if args.contains("-allocprobe") { Memory.probeAllocator() }
            if args.contains("-thumbnails") { await measureThumbnails() }
        }
    }

    /// Floating over the grid: import, select, and what is going on.
    /// The import's progress, in the middle of the grid: a bar while it can
    /// be counted, else a spinner, what it is doing, and Cancel.
    private func importCard(_ text: String) -> some View {
        VStack(spacing: 14) {
            if let p = model.importProgress, p.total > 0 {
                ProgressView(value: Double(p.done), total: Double(p.total))
                    .frame(width: 240)
            } else {
                ProgressView().controlSize(.large)
            }
            Text(text).font(.callout).multilineTextAlignment(.center)
            if let job = model.importJob {
                Button("Cancel", role: .cancel) { job.cancel() }
            }
        }
        .padding(24)
        .frame(maxWidth: 320)
        .background(Theme.raised, in: RoundedRectangle(cornerRadius: 16))
        .overlay(RoundedRectangle(cornerRadius: 16).stroke(Theme.line, lineWidth: 0.5))
        .padding()
    }

    @ViewBuilder private var bottomPill: some View {
        HStack(spacing: 14) {
            if let importing = model.importing {
                ProgressView().tint(.white)
                Text(importing).lineLimit(1)
                if let job = model.importJob { Button("Cancel") { job.cancel() } }
            } else {
                Menu {
                    Button("From a Card or Drive…", systemImage: "sdcard") { pickFolder = true; picking = true }
                    Button("From Files…", systemImage: "doc") { pickFolder = false; picking = true }
                    Button("From Photos…", systemImage: "photo.on.rectangle") { pickingPhotos = true }
                } label: {
                    Label("Import", systemImage: "square.and.arrow.down")
                }
                .disabled(model.library == nil)
                Divider().frame(height: 18)
                Button(selecting ? "Done" : "Select", systemImage: selecting ? "checkmark.circle" : "checkmark.circle") {
                    selecting.toggle()
                    selected = []
                }
                if let note = model.notice ?? pasteNote {
                    Divider().frame(height: 18)
                    Text(note).foregroundStyle(Theme.secondaryText).lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 18).padding(.vertical, 10)
        .background(Theme.raised, in: Capsule())
        .overlay(Capsule().stroke(Theme.line, lineWidth: 0.5))
        .frame(maxWidth: 560)
    }

    /// Opens Compare or Survey on the photos chosen, in the grid's order.
    private func choose(_ mode: ChoosingRequest.Mode) {
        model.choosing = ChoosingRequest(mode: mode, ids: model.shown.map(\.id).filter(selected.contains))
    }

    /// Makes every thumbnail, four at a time, and prints how long it took:
    /// the `-thumbnails` launch option.
    private func measureThumbnails() async {
        let start = ContinuousClock.now
        await withTaskGroup(of: Void.self) { group in
            for photo in model.photos { group.addTask { _ = await model.thumbnail(photo, edge: 340) } }
        }
        let memory = Memory.footprint()
        print("THUMBNAILS \(model.photos.count) in \(start.duration(to: .now)); memory \(Memory.mb(memory.current)), peak \(Memory.mb(memory.peak))")
    }
}

extension LibraryPhoto: Identifiable {}
extension ImportChoice: Identifiable {}

struct ThumbnailCell: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var image: UIImage?
    @State private var loaded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Color(white: 0.15)
                .aspectRatio(1, contentMode: .fit)
                .overlay {
                    if let image {
                        Image(uiImage: image).resizable().scaledToFit()
                    } else if loaded {
                        Image(systemName: "photo").font(.largeTitle).foregroundStyle(.secondary)
                    } else {
                        ProgressView()
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: 6))
            HStack(spacing: 4) {
                if !photo.label.isEmpty {
                    Circle().fill(labelColor(photo.label)).frame(width: 8, height: 8)
                }
                if photo.flag != 0 {
                    Image(systemName: photo.flag > 0 ? "flag.fill" : "xmark.circle.fill")
                        .font(.caption2).foregroundStyle(photo.flag > 0 ? Color.primary : Color.red)
                }
                Text(photo.title).font(.caption).lineLimit(1)
                Spacer()
                Stars(rating: photo.rating).font(.caption2)
            }
        }
        .task(id: photo.id) {
            image = await model.thumbnail(photo, edge: 340)
            loaded = true
        }
    }
}

struct Stars: View {
    let rating: Int32
    var body: some View {
        HStack(spacing: 1) {
            ForEach(1...5, id: \.self) { n in
                Image(systemName: n <= rating ? "star.fill" : "star")
                    .foregroundStyle(n <= rating ? .yellow : .secondary)
            }
        }
    }
}

/// A photo's long-press menu: rating, flag, label and Quick Collection.
struct PhotoMenu: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    var body: some View {
        Menu("Rating") {
            ForEach(0...5, id: \.self) { n in
                Button(n == 0 ? "No rating" : String(repeating: "★", count: n)) {
                    model.setRating(photo, to: Int32(n))
                }
            }
        }
        Menu("Flag") {
            Button("Pick", systemImage: "flag.fill") { model.setFlag(photo, to: 1) }
            Button("Reject", systemImage: "xmark.circle") { model.setFlag(photo, to: -1) }
            Button("Unflag", systemImage: "flag.slash") { model.setFlag(photo, to: 0) }
        }
        Menu("Label") {
            ForEach(labels, id: \.self) { label in
                Button(label) { model.setLabel(photo, to: label) }
            }
            Button("None") { model.setLabel(photo, to: "") }
        }
        Button("Info…", systemImage: "info.circle") { model.info = InfoRequest(ids: [photo.id]) }
        CopyMenu(photo: photo, model: model, renaming: Bindable(model).renamingCopy, removing: Bindable(model).removingCopy)
        StackMenu(photo: photo, model: model)
        Divider()
        AlbumMenu(ids: [photo.id], model: model)
        if model.inQuickCollection(photo) {
            Button("Remove from Quick Collection", systemImage: "circle.slash") { model.setQuickCollection([photo.id], false) }
        } else {
            Button("Add to Quick Collection", systemImage: "circle.inset.filled") { model.setQuickCollection([photo.id], true) }
        }
        Divider()
        FileMenu(ids: [photo.id], photo: photo, model: model)
    }
}

/// Lightroom's colour labels.
let labels = ["Red", "Yellow", "Green", "Blue", "Purple"]

func labelColor(_ label: String) -> Color {
    switch label {
    case "Red": .red
    case "Yellow": .yellow
    case "Green": .green
    case "Blue": .blue
    case "Purple": .purple
    default: .gray
    }
}

/// All Photographs, the Quick Collection and the catalog's collections.
/// The live editor for one photo of the catalog, with its rating. Leaving
/// saves the edit first; if that fails, the editor stays and says why.
struct EditorView: View {
    let photo: LibraryPhoto
    let model: LibraryModel
    @State private var live = LiveModel()
    @State private var quick = QuickExport()
    @State private var saveFailed = false
    /// The move to another photo a failed save held back; `nil`, leaving.
    @State private var heldMove: Int?
    /// The photo on screen: the one opened, then wherever a swipe went.
    @State private var current: LibraryPhoto?
    @Environment(\.dismiss) private var dismiss

    private var shown: LibraryPhoto { current ?? photo }
    private var rating: Int32 { model.photos.first { $0.id == shown.id }?.rating ?? shown.rating }
    private var flag: Int32 { model.photos.first { $0.id == shown.id }?.flag ?? shown.flag }
    /// Where the photo is in the grid's order, as the grid's filters and sort
    /// have it now.
    private var position: (index: Int, count: Int)? {
        let order = model.shown
        return order.firstIndex { $0.id == shown.id }.map { ($0, order.count) }
    }

    var body: some View {
        Group {
            if let library = model.library {
                LiveEditor(
                    photo: shown, library: library, model: live, rating: rating, flag: flag,
                    setRating: { model.setRating(shown, to: $0) }, setFlag: { model.setFlag(shown, to: $0) },
                    showInfo: { model.info = InfoRequest(ids: [shown.id]) })
            }
        }
        .navigationTitle(shown.title)
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden()
        .onAppear { live.onSwipe = { go(by: $0) } }
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button("Library", systemImage: "chevron.backward") {
                    heldMove = nil
                    if live.save() { dismiss() } else { saveFailed = true }
                }
            }
            ToolbarItem(placement: .navigation) {
                HStack(spacing: 2) {
                    Button("Previous Photo", systemImage: "arrow.left") { go(by: -1) }
                        .disabled((position?.index ?? 0) == 0)
                    if let position { Text("\(position.index + 1) of \(position.count)").font(.caption).monospacedDigit() }
                    Button("Next Photo", systemImage: "arrow.right") { go(by: 1) }
                        .disabled(position.map { $0.index + 1 >= $0.count } ?? true)
                }
                .labelStyle(.iconOnly)
            }
            ToolbarItemGroup(placement: .primaryAction) {
                Button("Undo", systemImage: "arrow.uturn.backward") { live.undo() }
                    .disabled(!(live.edit?.canUndo ?? false))
                Button("Redo", systemImage: "arrow.uturn.forward") { live.redo() }
                    .disabled(!(live.edit?.canRedo ?? false))
                BeforeAfterMenu(model: live)
                // As Lightroom: Save to Device at once, and Share's menu.
                // Each export reads the edit from the catalog, so it saves first.
                Button("Save to Device", systemImage: "square.and.arrow.down") {
                    if live.save(), let library = model.library { quick.run(.save, library: library, ids: [shown.id]) }
                }
                .disabled(quick.running)
                Menu("Share", systemImage: "square.and.arrow.up") {
                    Button("Save to Device", systemImage: "square.and.arrow.down") {
                        if live.save(), let library = model.library { quick.run(.save, library: library, ids: [shown.id]) }
                    }
                    Button("Share", systemImage: "square.and.arrow.up") {
                        if live.save(), let library = model.library { quick.run(.share, library: library, ids: [shown.id]) }
                    }
                    Divider()
                    Button {
                        if live.save() { live.exporting = true }
                    } label: {
                        Label {
                            Text("Export As…")
                            Text("Choose file type, size and more")
                        } icon: { Image(systemName: "rectangle.portrait.and.arrow.right") }
                    }
                }
                .disabled(quick.running)
                Menu("More", systemImage: "ellipsis") {
                    Button("Copy Settings…", systemImage: "doc.on.doc") { live.copying = true }
                    Button("Paste Settings", systemImage: "doc.on.clipboard") { live.paste() }
                        .disabled(!(live.copied || (model.library?.hasCopiedSettings() ?? false)))
                    Button("Paste from Previous", systemImage: "arrow.uturn.backward.square") {
                        _ = live.live?.pastePrevious()
                        live.refresh()
                    }
                    .disabled(!(live.live?.hasPrevious() ?? false))
                    Button("Reset Settings", systemImage: "arrow.counterclockwise") { live.live?.reset(); live.refresh() }
                    Divider()
                    Button("Create Virtual Copy", systemImage: "plus.square.on.square") { copyAndOpen() }
                    Button("Info…", systemImage: "info.circle") { model.info = InfoRequest(ids: [shown.id]) }
                    Divider()
                    SwiftUI.Toggle("Info Overlay", systemImage: "text.below.photo", isOn: Bindable(live).infoOverlay)
                    SwiftUI.Toggle("Camera Thumbnail", systemImage: "camera", isOn: Bindable(live).cameraThumbnail)
                    Button("Set as Reference Photo", systemImage: "pin") {
                        live.reference = shown
                    }
                    SwiftUI.Toggle("Reference View", systemImage: "rectangle.split.2x1", isOn: Bindable(live).referenceView)
                        .disabled(live.reference == nil || live.reference?.id == shown.id)
                }
            }
        }
        .toolbarBackground(Theme.canvas, for: .navigationBar)
        .toolbarBackground(.visible, for: .navigationBar)
        .overlay(alignment: .top) {
            if quick.running || quick.said != nil {
                HStack(spacing: 8) {
                    if quick.running { ProgressView().controlSize(.small) }
                    Text(quick.said ?? "Exporting…").font(.callout)
                }
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(.regularMaterial, in: Capsule())
                .padding(.top, 10)
            }
        }
        .sheet(isPresented: Binding(get: { !quick.files.isEmpty }, set: { if !$0 { quick.files = [] } })) {
            ShareSheet(items: quick.files)
        }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.willResignActiveNotification)) { _ in
            live.save()
        }
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.didEnterBackgroundNotification)) { _ in
            live.save()
        }
        .alert("The edit couldn't be saved", isPresented: $saveFailed) {
            Button("Try Again") {
                if let by = heldMove { go(by: by) } else if live.save() { dismiss() } else { saveFailed = true }
            }
            Button("Stay", role: .cancel) {}
        } message: {
            Text(live.edit?.saveError ?? "")
        }
    }

    /// Lightroom's Create Virtual Copy from the editor: saves, makes the copy
    /// and goes on editing it.
    private func copyAndOpen() {
        guard live.save() else { heldMove = nil; saveFailed = true; return }
        Task { if let copy = await model.createVirtualCopy(of: shown) { current = copy } }
    }

    /// Moves `by` photos along the grid's order, saving this one's edit
    /// first: a failed save stays here and says so.
    private func go(by: Int) {
        guard let position, model.shown.indices.contains(position.index + by) else { return }
        guard live.save() else { heldMove = by; saveFailed = true; return }
        heldMove = nil
        current = model.shown[position.index + by]
    }
}
